-- ============================================================
-- FMap 数据库 Schema v9 增量（M5 性能与使用性完善）
-- 由 crates/database/src/lib.rs 的 v9 迁移阶段（include_str!）在同一
-- 事务内执行；tests/validate-schema.mjs 拼接验证。
--
-- 原则：全部非破坏（只有 CREATE INDEX IF NOT EXISTS）；历史数据不动。
-- ============================================================

-- 历史追踪页全局时间线查询（WHERE rel_path IS NOT NULL ORDER BY ts DESC
-- LIMIT ?）：现有 (entity_id,ts)/(path_id,ts) 均为局部索引，无法服务
-- 全局 ORDER BY——事件量增长后为全表排序。补全局时间索引。
CREATE INDEX IF NOT EXISTS idx_events_ts ON file_events(ts DESC, id DESC);

-- 关联候选列表（按创建时间倒序浏览）。
CREATE INDEX IF NOT EXISTS idx_corr_created ON correlation_candidates(created_at DESC);

-- 下载记录按导入时间浏览。
CREATE INDEX IF NOT EXISTS idx_bdl_imported ON browser_downloads(imported_at DESC);
