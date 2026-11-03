-- ============================================================
-- TraceNest 数据库 Schema v1（唯一权威来源）
-- 依据 docs/architecture/DATABASE.md 的 DDL 草案整理。
-- 本文件同时被两处使用：
--   1) crates/database/src/lib.rs 通过 include_str! 在迁移中执行
--   2) tests/validate-schema.mjs 用 node:sqlite 真实验证（无需 Rust 工具链）
-- 原则：只存元数据，绝不存文件内容；所有表可空库初始化。
-- ============================================================

-- 应用元数据（schema 版本、规则包状态、应用标记）
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- 卷（磁盘）
CREATE TABLE IF NOT EXISTS volumes (
  id INTEGER PRIMARY KEY,
  root TEXT NOT NULL UNIQUE,          -- 'C:\' / 'D:\'
  label TEXT,
  total_bytes INTEGER,
  free_bytes INTEGER,
  fs_type TEXT,                        -- NTFS / exFAT / ...
  is_removable INTEGER NOT NULL DEFAULT 0,
  first_seen INTEGER NOT NULL          -- unix ms
);

-- 扫描代（每次全量/增量扫描一个代号）
CREATE TABLE IF NOT EXISTS scans (
  id INTEGER PRIMARY KEY,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  status TEXT NOT NULL DEFAULT 'running',  -- running|done|cancelled|failed|partial
  roots TEXT NOT NULL,                     -- JSON 数组
  files_scanned INTEGER,
  dirs_scanned INTEGER,
  errors_count INTEGER,
  app_version TEXT
);

-- 路径节点（文件与目录统一存储，只存元数据）
CREATE TABLE IF NOT EXISTS paths (
  id INTEGER PRIMARY KEY,             -- 稳定节点 id（同卷内唯一）
  volume_id INTEGER NOT NULL REFERENCES volumes(id),
  parent_id INTEGER REFERENCES paths(id),
  name TEXT NOT NULL,                 -- 不含分隔符
  depth INTEGER NOT NULL,
  is_dir INTEGER NOT NULL,
  size INTEGER NOT NULL DEFAULT 0,    -- 目录 = 子树聚合
  mtime INTEGER,
  ctime INTEGER,
  attrs INTEGER,                      -- 原始属性位（含 reparse/hardlink 标记）
  scan_id INTEGER NOT NULL REFERENCES scans(id),
  deleted_at INTEGER                  -- 增量扫描发现消失时填充（软删除）
);
CREATE INDEX IF NOT EXISTS idx_paths_parent ON paths(parent_id);
CREATE INDEX IF NOT EXISTS idx_paths_volume_name ON paths(volume_id, name);
CREATE INDEX IF NOT EXISTS idx_paths_size ON paths(volume_id, is_dir, size DESC);
CREATE INDEX IF NOT EXISTS idx_paths_mtime ON paths(volume_id, mtime);

-- 文件名全文搜索（FTS5，Rust 层批量维护，不用触发器）
CREATE VIRTUAL TABLE IF NOT EXISTS paths_fts USING fts5(name);

-- AI 工具
CREATE TABLE IF NOT EXISTS tools (
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,           -- 'claude-code' / 'codex' / ...
  name TEXT NOT NULL,
  vendor TEXT,
  color TEXT,
  builtin INTEGER NOT NULL DEFAULT 1  -- 1=内置规则 0=社区规则包
);

-- 规则包（版本化，本地导入）
CREATE TABLE IF NOT EXISTS rule_packs (
  id INTEGER PRIMARY KEY,
  pack_name TEXT NOT NULL,
  version TEXT NOT NULL,
  imported_at INTEGER NOT NULL,
  rule_count INTEGER,
  source TEXT NOT NULL DEFAULT 'builtin'  -- builtin | local-import
);

-- 规则（目录规则 + 信号文件规则）
CREATE TABLE IF NOT EXISTS rules (
  id INTEGER PRIMARY KEY,
  pack_id INTEGER REFERENCES rule_packs(id),
  tool_id INTEGER NOT NULL REFERENCES tools(id),
  kind TEXT NOT NULL,                 -- 'dir' | 'signal'
  os TEXT NOT NULL,                   -- 'win'|'mac'|'linux'|'*'
  pattern TEXT NOT NULL,              -- dir: 路径模板；signal: 文件名
  location TEXT,                      -- signal: 'project-root'|'anywhere'
  confidence TEXT NOT NULL,           -- 'verified'|'likely'
  kind_tag TEXT,                      -- install|config|cache|session|log|download|project
  risk_level TEXT,                    -- safe|caution|critical
  doc_url TEXT,
  source TEXT,
  UNIQUE(pack_id, tool_id, kind, pattern, location)
);

-- 归属结论（可按 scan 代整体重建）
CREATE TABLE IF NOT EXISTS attributions (
  path_id INTEGER NOT NULL REFERENCES paths(id),
  scan_id INTEGER NOT NULL,
  tool_id INTEGER REFERENCES tools(id),   -- NULL = unattributed
  confidence TEXT NOT NULL,           -- verified|likely|inferred|none
  score REAL NOT NULL DEFAULT 0,
  basis TEXT NOT NULL                 -- JSON: [{rule_id|signal|inherited-from:...}]
);
CREATE INDEX IF NOT EXISTS idx_attr_path ON attributions(path_id);
CREATE INDEX IF NOT EXISTS idx_attr_tool ON attributions(scan_id, tool_id, confidence);

-- 预聚合：目录子树内按工具的占用（treemap 着色/排行用）
CREATE TABLE IF NOT EXISTS agg_tool_size (
  path_id INTEGER NOT NULL,
  scan_id INTEGER NOT NULL,
  tool_id INTEGER,
  direct_bytes INTEGER NOT NULL DEFAULT 0,
  total_bytes INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (path_id, tool_id)
);

-- 实时事件（仅 watcher 覆盖的根）
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,                 -- create|modify|remove|rename_from|rename_to
  volume_id INTEGER REFERENCES volumes(id),
  path TEXT NOT NULL,
  size INTEGER,
  watch_root TEXT NOT NULL,
  scan_id INTEGER
);
CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts DESC);

-- 监控根
CREATE TABLE IF NOT EXISTS watch_roots (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  enabled INTEGER NOT NULL DEFAULT 1,
  added_at INTEGER NOT NULL
);
