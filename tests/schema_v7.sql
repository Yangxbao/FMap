-- ============================================================
-- FMap 数据库 Schema v7 增量（M3-C 工具行为关联）
-- 由 crates/database/src/lib.rs 的 v7 迁移阶段（include_str!）在同一
-- 事务内执行；tests/validate-schema.mjs 将本文件与此前各版拼接后验证。
--
-- 原则（与 v5/v6 一致）：
-- - 全部非破坏：只有 CREATE IF NOT EXISTS；历史数据一行不动；
-- - 候选属第二层（自动候选，可重建）：审查状态可变，事件/确认不可覆盖；
-- - 关联候选 = 时间+路径双上下文门槛的观察关联，**绝不携带来源行为**；
--   来源行为（ai_created/ai_modified...）唯一写入路径仍是用户确认（M3-A）。
-- ============================================================

-- 工具行为关联候选（M3-C）。
-- 挂点：差分事件（file_events append-only，引用稳定）；path_id 为展示冗余。
-- state 审查状态机：pending → confirmed / denied / ignored；
--   pending 也可由用户从已审查状态撤回（可撤销原则）。
--   重建（generation 递增）时：已审查行永久保留；pending 行按新计算刷新或清理。
-- confidence：likely（特征文件信号，窄窗口）/ possible（数据目录信号，宽窗口）。
-- evidence：JSON（core::correlation::CorrEvidence——工作区/信号/时间窗/差值，
--   全部为 FMap 自身观察到的元数据，不含文件内容、不含来源行为断言）。
CREATE TABLE IF NOT EXISTS correlation_candidates (
  id INTEGER PRIMARY KEY,
  event_id INTEGER NOT NULL REFERENCES file_events(id),
  path_id INTEGER REFERENCES paths(id),
  tool_id INTEGER NOT NULL REFERENCES tools(id),
  volume_id INTEGER NOT NULL REFERENCES volumes(id),
  rel_path TEXT NOT NULL,                    -- 卷根相对路径（展示用；事件行冗余）
  state TEXT NOT NULL DEFAULT 'pending',     -- pending | confirmed | denied | ignored
  confidence TEXT NOT NULL,                  -- likely | possible
  evidence TEXT NOT NULL,                    -- JSON: CorrEvidence
  generation INTEGER NOT NULL,               -- 候选代号（重建递增）
  created_at INTEGER NOT NULL,               -- unix ms
  reviewed_at INTEGER,                       -- 用户最近一次审查时刻（pending 时 NULL）
  review_note TEXT                           -- 用户审查备注（可选）
);
-- (事件, 工具) 唯一：重建 INSERT OR IGNORE 天然保留已审查行
CREATE UNIQUE INDEX IF NOT EXISTS ux_corr_event_tool
  ON correlation_candidates(event_id, tool_id);
CREATE INDEX IF NOT EXISTS idx_corr_state ON correlation_candidates(state, id DESC);
CREATE INDEX IF NOT EXISTS idx_corr_volume ON correlation_candidates(volume_id);
