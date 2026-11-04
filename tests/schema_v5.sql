-- ============================================================
-- FMap 数据库 Schema v5 增量（M3-A 文件资产基础模型）
-- 由 crates/database/src/lib.rs 的 v5 迁移阶段（include_str!）在同一
-- 事务内执行；tests/validate-schema.mjs 将本文件与 tests/schema.sql
-- 拼接后做 DDL 级验证。
--
-- 原则：
-- - 全部非破坏：只有 CREATE IF NOT EXISTS 与由 Rust ensure_column
--   执行的补列（volumes.volume_guid / attributions.entity_id）；
-- - 历史数据（paths / attributions 等 M1/M2 数据）一行不动；
-- - 三层分模型：原始事件与证据 → 自动候选 → 用户确认，分开存储；
--   UNKNOWN 永久合法，绝不编造来源。
-- ============================================================

-- 文件身份：卷 + FileId 主身份 + 弱身份降级链。
-- 身份键 = (volume_id, file_id)，不含盘符（盘符可变，卷 GUID 才稳定，
-- GUID 存于 volumes.volume_guid）。FileId（NTFS MFT 引用号）删除后可能
-- 被重用（微软文档明确警告），因此必须配合 (check_size, check_mtime)
-- 校验位；"消失 + 同 ID 重现"以 file_id_reuse_suspected 事件标记待确认，
-- 绝不静默改写身份。
CREATE TABLE IF NOT EXISTS file_entities (
  id INTEGER PRIMARY KEY,
  volume_id INTEGER NOT NULL REFERENCES volumes(id),
  identity_kind TEXT NOT NULL,               -- 'file_id'（主身份）| 'weak'（降级）
  file_id INTEGER,                           -- NTFS 64 位 FileId；弱身份时为 NULL
  check_size INTEGER,                        -- 校验位：size + mtime 组合
  check_mtime INTEGER,
  weak_path TEXT,                            -- 弱身份键：卷内相对路径（'/' 分隔）
  current_path_id INTEGER REFERENCES paths(id),  -- 最新扫描代中的对应路径节点（未映射为 NULL）
  first_seen_scan_id INTEGER REFERENCES scans(id),
  last_seen_scan_id INTEGER REFERENCES scans(id),
  first_seen_at INTEGER NOT NULL,            -- unix ms
  last_seen_at INTEGER NOT NULL,
  deleted_at INTEGER                         -- M3-B 差分确认消失时填充（软删除）
);
-- 部分唯一索引：主身份与弱身份分别保证同一卷内键唯一
CREATE UNIQUE INDEX IF NOT EXISTS ux_entities_file_id
  ON file_entities(volume_id, file_id) WHERE identity_kind='file_id' AND file_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_entities_weak
  ON file_entities(volume_id, weak_path) WHERE identity_kind='weak' AND weak_path IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_entities_volume ON file_entities(volume_id);
CREATE INDEX IF NOT EXISTS idx_entities_path ON file_entities(current_path_id);

-- 原始证据（独立表）：自动候选只引用证据 id，绝不复写证据本身。
-- kind: user_statement | tool_event | process | browser_download | scan_diff
CREATE TABLE IF NOT EXISTS source_evidence (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  origin TEXT NOT NULL,                      -- 产生者：'user' / 工具 key / 浏览器名
  collected_at INTEGER NOT NULL,             -- unix ms
  scope TEXT,                                -- 适用范围与限制（诚实声明证据能力边界）
  payload TEXT NOT NULL                      -- JSON 字面数据（仅元数据，不含文件正文）
);

-- 原始事件（append-only）：一经写入不可被自动判断覆盖。
-- M3-A 产生：source_confirmed / source_corrected / file_id_reuse_suspected；
-- M3-B 起产生：file_discovered / file_modified 等差分事件。
CREATE TABLE IF NOT EXISTS file_events (
  id INTEGER PRIMARY KEY,
  entity_id INTEGER NOT NULL REFERENCES file_entities(id) ON DELETE CASCADE,
  ts INTEGER NOT NULL,                       -- unix ms
  kind TEXT NOT NULL,
  evidence_id INTEGER REFERENCES source_evidence(id),
  scan_id INTEGER,
  detail TEXT                                -- JSON 补充（如重用嫌疑的前后校验值）
);
CREATE INDEX IF NOT EXISTS idx_events_entity ON file_events(entity_id, ts DESC);

-- 自动来源候选（可整体重建：规则/证据更新后按 generation DELETE+重算）。
-- M3-A 阶段无自动产生者（语义红线：时间共现/位置 ≠ 来源行为）；表结构与
-- 重建代号机制先落地，供 M3-C/M3-D 使用。
CREATE TABLE IF NOT EXISTS source_candidates (
  id INTEGER PRIMARY KEY,
  entity_id INTEGER NOT NULL REFERENCES file_entities(id) ON DELETE CASCADE,
  behavior TEXT NOT NULL,                    -- ai_created | ai_modified | ai_downloaded | ...
  tool_id INTEGER REFERENCES tools(id),
  confidence TEXT NOT NULL,
  evidence_ids TEXT NOT NULL DEFAULT '[]',   -- JSON 数组：source_evidence.id 列表
  generation INTEGER NOT NULL,               -- 候选代号；重建时整代替换
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_candidates_entity ON source_candidates(entity_id);

-- 用户确认 / 修正（append-only 保留修正历史）：最高可信，覆盖候选展示。
-- action: confirm | correct | revoke（revoke 时 behavior = 'unknown'）
CREATE TABLE IF NOT EXISTS source_confirmations (
  id INTEGER PRIMARY KEY,
  entity_id INTEGER NOT NULL REFERENCES file_entities(id) ON DELETE CASCADE,
  action TEXT NOT NULL,
  behavior TEXT NOT NULL,
  note TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_confirmations_entity ON source_confirmations(entity_id);
