-- ============================================================
-- FMap 数据库 Schema v6 增量（M3-B1 基础文件事件追踪）
-- 由 crates/database/src/lib.rs 的 v6 迁移阶段在**同一事务**内执行；
-- tests/validate-schema.mjs 将本文件与 v1 基线/v5 增量拼接验证。
--
-- 内容：
--   1) scan_diff_runs 差分执行台账（先建——file_events.diff_run_id 依赖它）
--   2) file_events 重建（挂点可空化 + 逻辑路径列 + 表级 CHECK + diff_run_id）
--   3) 旧事件数据搬迁（volume 经 实体→当前路径 推导；取不到为 NULL，
--      由 CHECK 允许 entity-only 行——身份域事件无逻辑路径键是合法历史）
--
-- 原则：非破坏（台账新建；file_events 重建保数据）；历史行不删除；
--       身份域与观察域分离（观察域事件在身份撤销时保留）。
-- ============================================================

-- 差分对执行台账：游标的唯一事实来源（派生），跳过/超限/完成统一记录。
-- status: completed | skipped_scope_mismatch | skipped_over_limit | pending_over_limit
-- 不变量（应用层校验）：各卷台账行构成从基线起的连续前缀；
-- pending_over_limit 行存在时阻塞该卷后续差分（选对算法）。
CREATE TABLE IF NOT EXISTS scan_diff_runs (
  id INTEGER PRIMARY KEY,
  volume_id INTEGER NOT NULL REFERENCES volumes(id),
  from_scan_id INTEGER NOT NULL REFERENCES scans(id),
  to_scan_id INTEGER NOT NULL REFERENCES scans(id),
  status TEXT NOT NULL,
  events_count INTEGER NOT NULL DEFAULT 0,
  ambiguous_count INTEGER NOT NULL DEFAULT 0,   -- 本对因名称规范化歧义暂缓追踪的路径数
  scope_json TEXT,                              -- 研判时使用的双代 scope 快照（跳过/超限时存 prev/curr 供人工复查）
  skip_reason TEXT,                             -- skipped_* 时的人类可读原因
  created_at INTEGER NOT NULL,
  UNIQUE(volume_id, to_scan_id)
);
CREATE INDEX IF NOT EXISTS idx_runs_vol_to ON scan_diff_runs(volume_id, to_scan_id);

-- file_events 重建（临时名，末尾 RENAME）。
-- 与 v5 的差异：
--   entity_id 可空（差分事件为 path-only）且**无 CASCADE**（身份撤销删实体时，
--   观察域事件由撤销流程先解除引用而保留——绝不级联丢失）；
--   path_id 可空（差分事件挂点）；
--   volume_id/rel_path/rel_key（逻辑路径键；旧身份域事件经推导填充，取不到为 NULL）；
--   diff_run_id（差分对台账强关联）；
--   表级 CHECK：挂点至少其一；path 挂点必须有卷。
DROP TABLE IF EXISTS file_events_v6;   -- 仅清除本迁移自己的临时表名残留（原子事务下不应存在）
CREATE TABLE file_events_v6 (
  id INTEGER PRIMARY KEY,
  entity_id INTEGER REFERENCES file_entities(id),
  path_id INTEGER REFERENCES paths(id),
  volume_id INTEGER REFERENCES volumes(id),
  rel_path TEXT,                               -- 原样相对路径（展示用；基准=卷登记根）
  rel_key TEXT,                                -- 规范化匹配键（大写化+'/'分隔；差分聚合用；差分事件必填——应用层保证）
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,
  evidence_id INTEGER REFERENCES source_evidence(id),
  diff_run_id INTEGER REFERENCES scan_diff_runs(id),
  scan_id INTEGER,
  detail TEXT,
  CHECK (entity_id IS NOT NULL OR path_id IS NOT NULL),
  CHECK (path_id IS NULL OR volume_id IS NOT NULL)
);
-- 旧事件搬迁：身份域事件（entity 挂点）原样保留；volume 尽力推导（entity→当前路径→卷）。
INSERT INTO file_events_v6(id, entity_id, path_id, volume_id, rel_path, rel_key,
                           ts, kind, evidence_id, diff_run_id, scan_id, detail)
SELECT e.id, e.entity_id, NULL,
       (SELECT p.volume_id FROM file_entities fe
          LEFT JOIN paths p ON fe.current_path_id = p.id
         WHERE fe.id = e.entity_id),
       NULL, NULL, e.ts, e.kind, e.evidence_id, NULL, e.scan_id, e.detail
  FROM file_events e;
DROP TABLE file_events;
ALTER TABLE file_events_v6 RENAME TO file_events;

CREATE INDEX IF NOT EXISTS idx_events_entity ON file_events(entity_id, ts DESC);
CREATE UNIQUE INDEX IF NOT EXISTS ux_events_kind_path_scan
  ON file_events(kind, path_id, scan_id) WHERE path_id IS NOT NULL AND scan_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_events_vol_relkey ON file_events(volume_id, rel_key, id DESC);
CREATE INDEX IF NOT EXISTS idx_events_path ON file_events(path_id, ts DESC);
CREATE INDEX IF NOT EXISTS idx_events_run ON file_events(diff_run_id);
