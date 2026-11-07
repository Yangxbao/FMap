-- ============================================================
-- FMap 数据库 Schema v8 增量（M3-D 浏览器下载记录接入）
-- 由 crates/database/src/lib.rs 的 v8 迁移阶段（include_str!）在同一
-- 事务内执行；tests/validate-schema.mjs 拼接验证。
--
-- 原则：
-- - 全部非破坏；历史数据一行不动；
-- - 授权模型：browser_download_sources 无行 = 功能完全关闭（默认关闭）；
--   逐源（浏览器 × 档案）授权，可暂停、可撤销（撤销=删除该源全部数据）；
-- - 最小信息：只存实现下载关联所必需的字段（目标路径/URL/状态/大小/时间/
--   是否打开）——**不存** referrer / tab_url / site_url 等浏览上下文；
-- - 下载记录 × 扫描结果只能建立**候选关联**（match_state），不认定来源。
-- ============================================================

-- 下载源授权（逐浏览器 × 逐档案；增行=授权，删行=撤销）。
CREATE TABLE IF NOT EXISTS browser_download_sources (
  id INTEGER PRIMARY KEY,
  browser TEXT NOT NULL,               -- 'chrome' | 'edge'
  profile TEXT NOT NULL,               -- 档案目录名（Default / Profile 1 / …）
  history_path TEXT NOT NULL,          -- 授权时登记的 History 文件路径（只读复制源）
  enabled INTEGER NOT NULL DEFAULT 1,  -- 暂停开关（暂停不删数据）
  added_at INTEGER NOT NULL,
  last_import_at INTEGER,
  UNIQUE(browser, profile)
);

-- 导入的下载记录（来自 History 副本的 downloads 表；最小字段集）。
CREATE TABLE IF NOT EXISTS browser_downloads (
  id INTEGER PRIMARY KEY,
  source_id INTEGER NOT NULL REFERENCES browser_download_sources(id) ON DELETE CASCADE,
  download_id INTEGER NOT NULL,        -- 浏览器侧 downloads.id
  target_path TEXT NOT NULL,           -- 下载目标绝对路径
  url TEXT,                            -- 下载 URL（downloads_url_chains 首项）
  state TEXT NOT NULL,                 -- complete|in_progress|cancelled|interrupted|unknown
  total_bytes INTEGER,
  received_bytes INTEGER,
  mime_type TEXT,
  start_time INTEGER,                  -- unix ms
  end_time INTEGER,                    -- unix ms
  opened INTEGER NOT NULL DEFAULT 0,
  imported_at INTEGER NOT NULL,
  -- 候选匹配（下载记录 × 扫描路径；只匹配不认定）：
  volume_id INTEGER REFERENCES volumes(id),
  path_id INTEGER REFERENCES paths(id),
  match_state TEXT NOT NULL DEFAULT 'unmatched',  -- unmatched|matched|stale
  UNIQUE(source_id, download_id)
);
CREATE INDEX IF NOT EXISTS idx_bdl_source ON browser_downloads(source_id, download_id);
CREATE INDEX IF NOT EXISTS idx_bdl_match ON browser_downloads(volume_id, path_id);
CREATE INDEX IF NOT EXISTS idx_bdl_state ON browser_downloads(source_id, match_state);
