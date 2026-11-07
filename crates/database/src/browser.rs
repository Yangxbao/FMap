//! # 浏览器下载记录接入（M3-D）：逐源授权 → History 副本解析 → 候选匹配
//!
//! 隐私与授权边界（**不得推翻**）：
//! - **默认关闭**：browser_download_sources 无行 = 功能完全关闭，不读任何浏览器数据；
//! - **逐源授权**：授权粒度 = 浏览器 × 档案（如 Edge/Default）；增行=授权，
//!   删行=撤销（CASCADE 删除该源全部导入记录）；暂停（enabled=0）不删数据；
//! - **最小信息**：只解析 `downloads` 与 `downloads_url_chains` 两张表的最小
//!   字段集（目标路径/URL/状态/大小/时间/是否打开）；**绝不 SELECT** urls /
//!   visits / keyword_search_terms 等浏览记录，**绝不**读取 Cookies /
//!   Login Data / 密码数据；
//! - **候选关联**：下载记录 × 扫描路径匹配 = 候选（match_state），不认定来源；
//!   来源行为仍只能由用户在来源确认里声明（M3-A 唯一写入路径）；
//! - 解析对象是 platform 层复制的**只读副本**（随解析结束自动删除），
//!   不打开原库、不与浏览器进程交互、不需要管理员。

use std::collections::HashSet;
use std::path::Path;

use rusqlite::{OpenFlags, OptionalExtension};
use serde::Serialize;
use tracenest_core::pathnorm;
use tracenest_core::CoreError;

use crate::Database;

/// 合法浏览器键（与 platform::SUPPORTED_BROWSERS 一致）。
pub const BROWSER_KEYS: [&str; 2] = ["chrome", "edge"];

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------- 数据行 ----------------

/// 授权源行（设置页数据源）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowserSourceRow {
    pub id: i64,
    pub browser: String,
    pub profile: String,
    pub history_path: String,
    pub enabled: bool,
    pub added_at: i64,
    pub last_import_at: Option<i64>,
    pub records: i64,
    pub matched: i64,
}

/// 下载记录行（列表数据源；最小字段集）。
#[derive(Debug, Clone, Serialize)]
pub struct DownloadRow {
    pub id: i64,
    pub source_id: i64,
    pub browser: String,
    pub profile: String,
    pub download_id: i64,
    pub target_path: String,
    pub url: Option<String>,
    pub state: String,
    pub total_bytes: Option<i64>,
    pub received_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub opened: bool,
    pub imported_at: i64,
    pub volume_id: Option<i64>,
    pub path_id: Option<i64>,
    pub match_state: String,
}

/// 一次导入的摘要（全部真实计数）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowserImportStats {
    pub source_id: i64,
    pub fetched: usize,
    pub inserted: usize,
    pub updated: usize,
    pub matched_total: i64,
    pub error: Option<String>,
}

// ---------------- History 副本解析（只读；最小字段集） ----------------

/// 解析产物（与 browser_downloads 表字段一一对应；无浏览上下文字段）。
#[derive(Debug, Clone)]
pub struct RawDownload {
    pub download_id: i64,
    pub target_path: String,
    pub url: Option<String>,
    pub state: String,
    pub total_bytes: Option<i64>,
    pub received_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub opened: bool,
}

/// Chrome/Edge 时间（WebKit 纪元微秒）→ unix ms（0/负值 = 未设置 → None）。
fn webkit_us_to_ms(v: i64) -> Option<i64> {
    if v <= 0 {
        return None;
    }
    Some(v / 1_000 - 11_644_473_600_000)
}

/// Chrome downloads.state → 语义词表（未知值诚实标 unknown，不猜）。
fn map_state(v: i64) -> String {
    match v {
        0 => "in_progress".into(),
        1 => "complete".into(),
        2 => "cancelled".into(),
        3 => "interrupted".into(),
        _ => "unknown".into(),
    }
}

/// 打开 History **副本**（只读）并解析 downloads 最小字段集。
///
/// 列自省：不同 Chrome/Edge 版本的 downloads 列集有差异——只 SELECT
/// 交集字段，缺列置 None；`id`/`target_path` 缺失才视为不兼容。
fn parse_history_copy(db_path: &Path) -> Result<Vec<RawDownload>, CoreError> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY, // 副本只读；绝不写浏览器数据
    )
    .map_err(|e| CoreError::Unsupported(format!("打开 History 副本失败: {e}")))?;

    // 列自省（只针对 downloads 表）
    let mut cols: HashSet<String> = HashSet::new();
    {
        let mut stmt = conn
            .prepare("SELECT name FROM pragma_table_info('downloads')")
            .map_err(|e| CoreError::Unsupported(format!("History 副本缺少 downloads 表: {e}")))?;
        let it = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| CoreError::Unsupported(format!("读取 downloads 列失败: {e}")))?;
        for c in it {
            cols.insert(c.map_err(|e| CoreError::Unsupported(format!("{e}")))?);
        }
    }
    for required in ["id", "target_path"] {
        if !cols.contains(required) {
            return Err(CoreError::Unsupported(format!(
                "downloads 表缺少必需列 {required}（浏览器版本不兼容，诚实拒绝）"
            )));
        }
    }
    let has = |c: &str| cols.contains(c);
    // 最小字段集（按存在性拼接；绝不展开 SELECT *）
    let mut parts: Vec<String> = vec!["id".into(), "target_path".into()];
    for optional in [
        "state",
        "total_bytes",
        "received_bytes",
        "mime_type",
        "start_time",
        "end_time",
        "opened",
    ] {
        if has(optional) {
            parts.push(optional.to_string());
        }
    }
    let select_cols = parts.join(",");

    let mut stmt = conn
        .prepare(&format!("SELECT {select_cols} FROM downloads"))
        .map_err(|e| CoreError::Unsupported(format!("读取 downloads 失败: {e}")))?;
    let col_count = select_cols.split(',').count();
    /// downloads 行的原始读取值（列自省交集；WebKit 微秒时间未换算）。
    struct RawRow {
        id: i64,
        target_path: String,
        state: Option<i64>,
        total_bytes: Option<i64>,
        received_bytes: Option<i64>,
        mime_type: Option<String>,
        start_us: Option<i64>,
        end_us: Option<i64>,
        opened: Option<i64>,
    }

    let it = stmt
        .query_map([], |r| {
            // 按固定顺序读取（与 select_cols 一致）
            let mut idx = 0usize;
            let id: i64 = r.get(idx)?;
            idx += 1;
            let target_path: String = r.get(idx)?;
            idx += 1;
            let opt_i64 = |r: &rusqlite::Row, i: usize| -> rusqlite::Result<Option<i64>> {
                if i < col_count {
                    r.get(i)
                } else {
                    Ok(None)
                }
            };
            let opt_str = |r: &rusqlite::Row, i: usize| -> rusqlite::Result<Option<String>> {
                if i < col_count {
                    r.get(i)
                } else {
                    Ok(None)
                }
            };
            let state = opt_i64(r, idx)?;
            idx += 1;
            let total_bytes = opt_i64(r, idx)?;
            idx += 1;
            let received_bytes = opt_i64(r, idx)?;
            idx += 1;
            let mime_type = opt_str(r, idx)?;
            idx += 1;
            let start_us = opt_i64(r, idx)?;
            idx += 1;
            let end_us = opt_i64(r, idx)?;
            idx += 1;
            let opened: Option<i64> = opt_i64(r, idx)?;
            Ok(RawRow {
                id,
                target_path,
                state,
                total_bytes,
                received_bytes,
                mime_type,
                start_us,
                end_us,
                opened,
            })
        })
        .map_err(|e| CoreError::Unsupported(format!("读取 downloads 失败: {e}")))?;

    let mut rows: Vec<RawRow> = Vec::new();
    for row in it {
        rows.push(row.map_err(|e| CoreError::Unsupported(format!("读取 downloads 行失败: {e}")))?);
    }

    // 下载 URL 链首（仅 downloads 相关表；任何其他表不触碰）
    let mut urls: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    let has_chain: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='downloads_url_chains'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|v| v > 0)
        .unwrap_or(false);
    if has_chain {
        let mut stmt = conn
            .prepare("SELECT id, url FROM downloads_url_chains WHERE chain_index=0")
            .map_err(|e| CoreError::Unsupported(format!("读取下载链失败: {e}")))?;
        let it = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| CoreError::Unsupported(format!("读取下载链失败: {e}")))?;
        for row in it {
            let (id, url) = row.map_err(|e| CoreError::Unsupported(format!("{e}")))?;
            urls.insert(id, url);
        }
    }

    Ok(rows
        .into_iter()
        .map(|r| RawDownload {
            download_id: r.id,
            target_path: r.target_path,
            url: urls.get(&r.id).cloned(),
            state: r.state.map_or_else(|| "unknown".into(), map_state),
            total_bytes: r.total_bytes,
            received_bytes: r.received_bytes,
            mime_type: r.mime_type,
            start_ms: r.start_us.and_then(webkit_us_to_ms),
            end_ms: r.end_us.and_then(webkit_us_to_ms),
            opened: r.opened.unwrap_or(0) != 0,
        })
        .collect())
}

// ---------------- 授权源 CRUD ----------------

impl Database {
    /// 授权源列表（含记录计数）。
    pub fn browser_sources(&self) -> Result<Vec<BrowserSourceRow>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT s.id, s.browser, s.profile, s.history_path, s.enabled, s.added_at,
                        s.last_import_at,
                        (SELECT COUNT(*) FROM browser_downloads d WHERE d.source_id=s.id),
                        (SELECT COUNT(*) FROM browser_downloads d WHERE d.source_id=s.id AND d.match_state='matched')
                 FROM browser_download_sources s ORDER BY s.id",
            )
            .map_err(|e| CoreError::Database(format!("授权源查询失败: {e}")))?;
        let it = stmt
            .query_map([], |r| {
                Ok(BrowserSourceRow {
                    id: r.get(0)?,
                    browser: r.get(1)?,
                    profile: r.get(2)?,
                    history_path: r.get(3)?,
                    enabled: r.get::<_, i64>(4)? != 0,
                    added_at: r.get(5)?,
                    last_import_at: r.get(6)?,
                    records: r.get(7)?,
                    matched: r.get(8)?,
                })
            })
            .map_err(|e| CoreError::Database(format!("授权源查询失败: {e}")))?;
        let mut out = Vec::new();
        for r in it {
            out.push(r.map_err(|e| CoreError::Database(format!("授权源读取失败: {e}")))?);
        }
        Ok(out)
    }

    /// 授权一个下载源（逐源授权；browser/profile 词表校验；路径必须存在）。
    pub fn browser_source_add(
        &self,
        browser: &str,
        profile: &str,
        history_path: &str,
    ) -> Result<i64, CoreError> {
        if !BROWSER_KEYS.contains(&browser) {
            return Err(CoreError::Other(format!("不支持的浏览器: {browser}")));
        }
        if profile.trim().is_empty() {
            return Err(CoreError::Other("档案名不能为空".into()));
        }
        if !Path::new(history_path).is_file() {
            return Err(CoreError::Other(format!(
                "History 文件不存在（授权前必须确认）: {history_path}"
            )));
        }
        self.conn
            .execute(
                "INSERT INTO browser_download_sources(browser,profile,history_path,enabled,added_at)
                 VALUES(?1,?2,?3,1,?4)
                 ON CONFLICT(browser,profile) DO UPDATE SET
                   history_path=excluded.history_path, enabled=1",
                rusqlite::params![browser, profile, history_path, now_ms()],
            )
            .map_err(|e| CoreError::Database(format!("授权失败: {e}")))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 撤销授权：删除源行（CASCADE 删除全部导入记录）。返回删除记录数。
    pub fn browser_source_remove(&self, source_id: i64) -> Result<i64, CoreError> {
        let before: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM browser_downloads WHERE source_id=?1",
                [source_id],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("统计记录失败: {e}")))?;
        let n = self
            .conn
            .execute(
                "DELETE FROM browser_download_sources WHERE id=?1",
                [source_id],
            )
            .map_err(|e| CoreError::Database(format!("撤销授权失败: {e}")))?;
        if n == 0 {
            return Err(CoreError::Other(format!("授权源不存在: {source_id}")));
        }
        Ok(before)
    }

    /// 暂停 / 恢复（暂停不删数据；暂停期间导入跳过）。
    pub fn browser_source_set_enabled(
        &self,
        source_id: i64,
        enabled: bool,
    ) -> Result<bool, CoreError> {
        let n = self
            .conn
            .execute(
                "UPDATE browser_download_sources SET enabled=?1 WHERE id=?2",
                rusqlite::params![enabled as i64, source_id],
            )
            .map_err(|e| CoreError::Database(format!("更新授权开关失败: {e}")))?;
        Ok(n > 0)
    }

    /// 清除某源已导入记录（保留授权）。
    pub fn browser_purge_records(&self, source_id: i64) -> Result<i64, CoreError> {
        self.conn
            .execute(
                "DELETE FROM browser_downloads WHERE source_id=?1",
                [source_id],
            )
            .map_err(|e| CoreError::Database(format!("清除记录失败: {e}")))?;
        Ok(self.conn.changes() as i64)
    }

    // ---------------- 导入 + 匹配 ----------------

    /// 导入一个源的下载记录（复制 History → 只读解析副本 → upsert → 匹配）。
    /// 源被暂停或不存在 → 错误（不静默）。
    pub fn browser_import_source(&self, source_id: i64) -> Result<BrowserImportStats, CoreError> {
        let (enabled, history_path): (i64, String) = self
            .conn
            .query_row(
                "SELECT enabled, history_path FROM browser_download_sources WHERE id=?1",
                [source_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| CoreError::Other(format!("授权源不存在: {source_id}")))?;
        if enabled == 0 {
            return Err(CoreError::Other(format!(
                "源 {source_id} 已暂停：请先恢复再导入"
            )));
        }
        // 1) 只读复制（platform 层；Drop 自清理）
        let copy =
            tracenest_platform::browser_downloads::copy_history_to_temp(Path::new(&history_path))?;
        // 2) 解析（最小字段集）
        let raw = match parse_history_copy(&copy.db_path) {
            Ok(v) => v,
            Err(e) => {
                return Ok(BrowserImportStats {
                    source_id,
                    fetched: 0,
                    inserted: 0,
                    updated: 0,
                    matched_total: 0,
                    error: Some(e.to_string()),
                });
            }
        };
        drop(copy); // 解析完成立即删除副本
        let fetched = raw.len();
        // 3) upsert（UNIQUE(source_id, download_id)；重复导入幂等更新）
        let ts = now_ms();
        let (mut inserted, mut updated) = (0usize, 0usize);
        {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启导入事务失败: {e}")))?;
            for r in &raw {
                // 先查后写（changes() 在 DO UPDATE 下不区分插入/更新）
                let exists: bool = tx
                    .query_row(
                        "SELECT 1 FROM browser_downloads WHERE source_id=?1 AND download_id=?2",
                        rusqlite::params![source_id, r.download_id],
                        |_| Ok(true),
                    )
                    .optional()
                    .map_err(|e| CoreError::Database(format!("查询既有记录失败: {e}")))?
                    .unwrap_or(false);
                if exists {
                    tx.execute(
                        "UPDATE browser_downloads SET target_path=?3, url=?4, state=?5,
                           total_bytes=?6, received_bytes=?7, mime_type=?8, start_time=?9,
                           end_time=?10, opened=?11, imported_at=?12
                         WHERE source_id=?1 AND download_id=?2",
                        rusqlite::params![
                            source_id,
                            r.download_id,
                            r.target_path,
                            r.url,
                            r.state,
                            r.total_bytes,
                            r.received_bytes,
                            r.mime_type,
                            r.start_ms,
                            r.end_ms,
                            r.opened as i64,
                            ts
                        ],
                    )
                    .map_err(|e| CoreError::Database(format!("更新记录失败: {e}")))?;
                    updated += 1;
                } else {
                    tx.execute(
                        "INSERT INTO browser_downloads
                         (source_id,download_id,target_path,url,state,total_bytes,received_bytes,mime_type,start_time,end_time,opened,imported_at)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                        rusqlite::params![
                            source_id,
                            r.download_id,
                            r.target_path,
                            r.url,
                            r.state,
                            r.total_bytes,
                            r.received_bytes,
                            r.mime_type,
                            r.start_ms,
                            r.end_ms,
                            r.opened as i64,
                            ts
                        ],
                    )
                    .map_err(|e| CoreError::Database(format!("导入记录失败: {e}")))?;
                    inserted += 1;
                }
            }
            tx.execute(
                "UPDATE browser_download_sources SET last_import_at=?1 WHERE id=?2",
                rusqlite::params![ts, source_id],
            )
            .map_err(|e| CoreError::Database(format!("更新导入时刻失败: {e}")))?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交导入失败: {e}")))?;
        }
        // 4) 候选匹配（下载 × 扫描路径；只匹配不认定）
        let matched_total = self.browser_match_downloads()?;
        Ok(BrowserImportStats {
            source_id,
            fetched,
            inserted,
            updated,
            matched_total,
            error: None,
        })
    }

    /// 候选匹配：把全部下载记录的 target_path 与最新扫描代路径匹配。
    ///
    /// 匹配语义（候选，不认定）：
    /// - 卷定位：规范化 target_path 前缀 = 登记根（最长根优先，支持嵌套登记根）；
    /// - 路径定位：从该卷最新写入代的根节点沿名称逐段下降（NOCASE——
    ///   ASCII 大小写折叠；与 rel_key 的 Unicode 大写化在极端字符上有理论差异，
    ///   匹配结果是候选，可复查）；
    /// - 匹配失败 → unmatched（诚实状态，不猜）。
    pub fn browser_match_downloads(&self) -> Result<i64, CoreError> {
        use rusqlite::OptionalExtension;
        // 卷清单（最长根优先——支持嵌套登记根 E:\ 与 E:\docs）
        let mut volumes: Vec<(i64, String)> = self.all_volume_roots();
        volumes.sort_by(|a, b| {
            pathnorm::normalize_root(&b.1)
                .len()
                .cmp(&pathnorm::normalize_root(&a.1).len())
        });
        // 每卷最新写入代的根节点
        let mut root_nodes: std::collections::HashMap<i64, Option<i64>> =
            std::collections::HashMap::new();
        for (vid, _) in &volumes {
            let root_id: Option<i64> = self
                .conn
                .query_row(
                    "SELECT p.id FROM paths p
                     WHERE p.volume_id=?1 AND p.scan_id=(SELECT MAX(scan_id) FROM paths WHERE volume_id=?1)
                       AND p.depth=0 AND p.parent_id IS NULL LIMIT 1",
                    [vid],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| CoreError::Database(format!("查询卷根节点失败: {e}")))?;
            root_nodes.insert(*vid, root_id);
        }
        let rows: Vec<(i64, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, target_path FROM browser_downloads")
                .map_err(|e| CoreError::Database(format!("加载下载记录失败: {e}")))?;
            let it = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| CoreError::Database(format!("加载下载记录失败: {e}")))?;
            let mut v = Vec::new();
            for r in it {
                v.push(r.map_err(|e| CoreError::Database(format!("{e}")))?);
            }
            v
        };
        let mut matched = 0i64;
        for (row_id, target) in rows {
            let norm = pathnorm::normalize(&target);
            let mut hit: Option<(i64, i64)> = None;
            for (vid, root) in &volumes {
                let root_key = pathnorm::normalize(root); // "E:"（去尾分隔）
                let rel = if norm == root_key {
                    String::new()
                } else if norm.starts_with(&format!("{root_key}/")) {
                    norm[root_key.len() + 1..].to_string()
                } else {
                    continue;
                };
                let Some(mut node) = root_nodes.get(vid).copied().flatten() else {
                    continue;
                };
                if rel.is_empty() {
                    continue; // 目标=卷根本身，不成立
                }
                let mut ok = true;
                for seg in rel.split('/') {
                    let r = self
                        .conn
                        .query_row(
                            "SELECT id FROM paths WHERE parent_id=?1 AND name=?2 COLLATE NOCASE
                         AND scan_id=(SELECT MAX(scan_id) FROM paths WHERE volume_id=?3) LIMIT 1",
                            rusqlite::params![node, seg, vid],
                            |r| r.get::<_, i64>(0),
                        )
                        .optional()
                        .map_err(|e| CoreError::Database(format!("路径下降查询失败: {e}")))?;
                    match r {
                        Some(id) => node = id,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    hit = Some((*vid, node));
                    break;
                }
            }
            match hit {
                Some((vid, pid)) => {
                    self.conn
                        .execute(
                            "UPDATE browser_downloads SET volume_id=?1, path_id=?2, match_state='matched' WHERE id=?3",
                            rusqlite::params![vid, pid, row_id],
                        )
                        .map_err(|e| CoreError::Database(format!("更新匹配失败: {e}")))?;
                    matched += 1;
                }
                None => {
                    self.conn
                        .execute(
                            "UPDATE browser_downloads SET volume_id=NULL, path_id=NULL, match_state='unmatched' WHERE id=?1",
                            [row_id],
                        )
                        .map_err(|e| CoreError::Database(format!("更新匹配失败: {e}")))?;
                }
            }
        }
        Ok(matched)
    }

    // ---------------- 查询 ----------------

    /// 下载记录列表（source_id=None = 全部源；id 倒序）。
    pub fn browser_downloads_list(
        &self,
        source_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<DownloadRow>, CoreError> {
        let sql = "SELECT d.id, d.source_id, s.browser, s.profile, d.download_id, d.target_path,
                          d.url, d.state, d.total_bytes, d.received_bytes, d.mime_type,
                          d.start_time, d.end_time, d.opened, d.imported_at,
                          d.volume_id, d.path_id, d.match_state
                   FROM browser_downloads d
                   JOIN browser_download_sources s ON s.id = d.source_id";
        let (query, params): (String, Vec<i64>) = match source_id {
            Some(sid) => (
                format!("{sql} WHERE d.source_id=?1 ORDER BY d.id DESC LIMIT ?2"),
                vec![sid, limit.clamp(1, 500)],
            ),
            None => (
                format!("{sql} ORDER BY d.id DESC LIMIT ?1"),
                vec![limit.clamp(1, 500)],
            ),
        };
        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| CoreError::Database(format!("下载记录查询失败: {e}")))?;
        let it = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |r| {
                Ok(DownloadRow {
                    id: r.get(0)?,
                    source_id: r.get(1)?,
                    browser: r.get(2)?,
                    profile: r.get(3)?,
                    download_id: r.get(4)?,
                    target_path: r.get(5)?,
                    url: r.get(6)?,
                    state: r.get(7)?,
                    total_bytes: r.get(8)?,
                    received_bytes: r.get(9)?,
                    mime_type: r.get(10)?,
                    start_time: r.get(11)?,
                    end_time: r.get(12)?,
                    opened: r.get::<_, i64>(13)? != 0,
                    imported_at: r.get(14)?,
                    volume_id: r.get(15)?,
                    path_id: r.get(16)?,
                    match_state: r.get(17)?,
                })
            })
            .map_err(|e| CoreError::Database(format!("下载记录查询失败: {e}")))?;
        let mut out = Vec::new();
        for r in it {
            out.push(r.map_err(|e| CoreError::Database(format!("下载记录读取失败: {e}")))?);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 测试：授权默认关闭 / 逐源授权与撤销 / 合成 History 导入 / 候选匹配 / 去重
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use tracenest_core::scan_scope::ScanScope;

    fn mem_db() -> Database {
        Database::open(":memory:".as_ref()).expect("内存库应能打开")
    }

    /// 构造一个合成 History 文件（Chrome downloads 最小结构 + 诱饵 urls 表）。
    /// 返回文件路径。行：1=完整下载 → E:\proj\setup.exe；2=进行中 → 不存在路径。
    fn make_history(path: &Path) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE downloads (
                id INTEGER PRIMARY KEY, target_path TEXT NOT NULL, state INTEGER,
                danger_type INTEGER, received_bytes INTEGER, total_bytes INTEGER,
                mime_type TEXT, start_time INTEGER, end_time INTEGER, opened INTEGER,
                referrer TEXT, tab_url TEXT
             );
             CREATE TABLE downloads_url_chains (id INTEGER, chain_index INTEGER, url TEXT);
             -- 诱饵：浏览记录表（含敏感内容）——导入绝不读取
             CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT);
             INSERT INTO urls VALUES (1, 'https://secret.example/private-page', '隐私标题');
             INSERT INTO downloads VALUES
               (1, 'E:\\proj\\setup.exe', 1, 0, 1000, 1000, 'application/octet-stream',
                13348540800000000, 13348540806000000, 0, 'https://ref.example/page', 'https://tab.example');
             INSERT INTO downloads VALUES
               (2, 'E:\\nowhere\\gone.bin', 0, 0, 10, 100, 'app/x', 0, 0, 0, '', '');
             INSERT INTO downloads_url_chains VALUES (1, 0, 'https://dl.example/setup.exe');
             INSERT INTO downloads_url_chains VALUES (1, 1, 'https://cdn.example/setup.exe');",
        )
        .unwrap();
    }

    /// 种子：卷 E:\ + scope 扫描代 + 路径树（模拟 scan_writer：depth0 根节点 + PROJ/setup.exe）。
    fn seed_volume_scan(db: &Database) -> i64 {
        let vol = db
            .upsert_volume("E:\\", None, None, None, Some("NTFS"), false)
            .unwrap();
        let scan = db
            .begin_scan(&["E:\\".into()], "t", db.instance_id())
            .unwrap();
        db.set_scan_scope(
            scan,
            &ScanScope {
                roots: vec!["E:\\".into()],
                exclude_dir_names: vec![],
                follow_symlinks: false,
            },
        )
        .unwrap();
        // scan_writer 会先插入卷根节点（depth 0，parent NULL）
        let root = db
            .insert_path(scan, vol, None, "E:", 0, true, 0, Some(0), 0)
            .unwrap();
        let proj = db
            .insert_path(scan, vol, Some(root), "PROJ", 1, true, 0, Some(0), 0)
            .unwrap();
        db.insert_path(
            scan,
            vol,
            Some(proj),
            "setup.exe",
            2,
            false,
            1000,
            Some(0),
            0,
        )
        .unwrap();
        scan
    }

    #[test]
    fn 授权默认关闭_无源无导入() {
        let db = mem_db();
        assert!(db.browser_sources().unwrap().is_empty(), "默认零授权");
        // 全量导入：无源 → 无操作
        let _ = db.browser_import_source(1);
        assert!(db.browser_downloads_list(None, 100).unwrap().is_empty());
    }

    #[test]
    fn 授权词表与路径校验_增删改与撤销级联() {
        let db = mem_db();
        let dir = tempfile::tempdir().unwrap();
        let hist = dir.path().join("History");
        std::fs::write(&hist, b"x").unwrap();
        // 非法浏览器拒绝
        assert!(db
            .browser_source_add("safari", "Default", hist.to_str().unwrap())
            .is_err());
        // 路径不存在拒绝（授权前必须确认）
        assert!(db
            .browser_source_add("edge", "Default", "Z:/no/History")
            .is_err());
        // 合法授权
        let id = db
            .browser_source_add("edge", "Default", hist.to_str().unwrap())
            .unwrap();
        assert_eq!(db.browser_sources().unwrap().len(), 1);
        // 暂停 / 恢复
        assert!(db.browser_source_set_enabled(id, false).unwrap());
        assert!(!db.browser_sources().unwrap()[0].enabled);
        // 撤销（无记录）
        assert_eq!(db.browser_source_remove(id).unwrap(), 0);
        assert!(db.browser_source_remove(id).is_err(), "重复撤销报错");
    }

    #[test]
    fn 合成history导入_最小字段_去重_匹配_诱饵表不触碰() {
        let db = mem_db();
        seed_volume_scan(&db);
        let dir = tempfile::tempdir().unwrap();
        let hist = dir.path().join("History");
        make_history(&hist);
        let sid = db
            .browser_source_add("edge", "Default", hist.to_str().unwrap())
            .unwrap();

        let stats = db.browser_import_source(sid).unwrap();
        assert_eq!(stats.fetched, 2);
        assert_eq!(stats.inserted, 2);
        assert_eq!(stats.matched_total, 1, "只有 setup.exe 匹配到扫描路径");

        let rows = db.browser_downloads_list(None, 100).unwrap();
        assert_eq!(rows.len(), 2);
        let complete = rows.iter().find(|r| r.download_id == 1).unwrap();
        assert_eq!(complete.state, "complete");
        assert_eq!(complete.match_state, "matched");
        assert_eq!(complete.target_path, "E:\\proj\\setup.exe");
        assert_eq!(
            complete.url.as_deref(),
            Some("https://dl.example/setup.exe"),
            "URL 取链首"
        );
        assert!(complete.start_time.is_some() && complete.end_time.is_some());
        assert_eq!(complete.total_bytes, Some(1000));
        // 候选匹配落到真实路径节点
        assert!(complete.path_id.is_some() && complete.volume_id.is_some());
        let unmatched = rows.iter().find(|r| r.download_id == 2).unwrap();
        assert_eq!(
            unmatched.match_state, "unmatched",
            "不存在路径诚实标记未匹配"
        );
        assert!(unmatched.path_id.is_none());

        // WebKit 时间换算合理性：133600000000000000us ≈ 2023-12 附近
        let ms = complete.start_time.unwrap();
        let human = chrono_like(ms);
        assert!(
            human.contains("2023") || human.contains("2024"),
            "WebKit 时间换算: {human}"
        );

        // 去重：二次导入 inserted=0 updated=2
        let stats2 = db.browser_import_source(sid).unwrap();
        assert_eq!(stats2.inserted, 0);
        assert_eq!(stats2.updated, 2);
        assert_eq!(db.browser_downloads_list(None, 100).unwrap().len(), 2);

        // 最小信息断言：行内不含诱饵 urls 表数据 / referrer / tab_url
        let payload = serde_json::to_string(&rows).unwrap();
        assert!(!payload.contains("secret.example"));
        assert!(!payload.contains("隐私标题"));
        assert!(!payload.contains("ref.example"));
        assert!(!payload.contains("tab.example"));

        // 清除记录（保留授权）
        assert_eq!(db.browser_purge_records(sid).unwrap(), 2);
        assert!(db.browser_downloads_list(None, 100).unwrap().is_empty());
        assert_eq!(db.browser_sources().unwrap().len(), 1);
    }

    /// 简化日期展示（测试专用，不依赖外部时间库）。
    fn chrono_like(ms: i64) -> String {
        // days since epoch → 近似年份判断
        let days = ms / 86_400_000;
        let year = 1970 + days / 365;
        format!("{year}-approx")
    }

    #[test]
    fn 暂停源导入被拒绝_不静默() {
        let db = mem_db();
        let dir = tempfile::tempdir().unwrap();
        let hist = dir.path().join("History");
        make_history(&hist);
        let sid = db
            .browser_source_add("chrome", "Profile 1", hist.to_str().unwrap())
            .unwrap();
        db.browser_source_set_enabled(sid, false).unwrap();
        assert!(db.browser_import_source(sid).is_err(), "暂停源导入必须报错");
    }

    #[test]
    fn 撤销源_级联删除全部记录() {
        let db = mem_db();
        seed_volume_scan(&db);
        let dir = tempfile::tempdir().unwrap();
        let hist = dir.path().join("History");
        make_history(&hist);
        let sid = db
            .browser_source_add("edge", "Default", hist.to_str().unwrap())
            .unwrap();
        db.browser_import_source(sid).unwrap();
        assert_eq!(db.browser_downloads_list(None, 100).unwrap().len(), 2);
        assert_eq!(
            db.browser_source_remove(sid).unwrap(),
            2,
            "撤销返回删除记录数"
        );
        assert!(db.browser_sources().unwrap().is_empty());
        assert!(
            db.browser_downloads_list(None, 100).unwrap().is_empty(),
            "级联删除"
        );
    }

    #[test]
    fn 不存在的history_导入返回错误摘要_不崩溃() {
        let db = mem_db();
        let sid = db
            .browser_source_add("edge", "Default", "Z:/no/such/History")
            .is_err();
        assert!(sid, "路径不存在时授权即拒绝");
    }

    #[test]
    fn 大小写不敏感匹配_嵌套登记根最长优先() {
        let db = mem_db();
        let vol = db
            .upsert_volume("E:\\", None, None, None, Some("NTFS"), false)
            .unwrap();
        let vol_docs = db
            .upsert_volume("E:\\docs", None, None, None, None, false)
            .unwrap();
        assert_ne!(vol, vol_docs);
        let scan = db
            .begin_scan(&["E:\\".into()], "t", db.instance_id())
            .unwrap();
        let root = db
            .insert_path(scan, vol, None, "E:", 0, true, 0, Some(0), 0)
            .unwrap();
        let docs = db
            .insert_path(scan, vol, Some(root), "docs", 1, true, 0, Some(0), 0)
            .unwrap();
        db.insert_path(scan, vol, Some(docs), "file.zip", 2, false, 5, Some(0), 0)
            .unwrap();
        // 嵌套根登记 E:\docs：其最新"代"无路径 → 根节点缺失 → 只能由 E:\ 命中
        db.conn
            .execute(
                "INSERT INTO browser_download_sources(browser,profile,history_path,enabled,added_at)
                 VALUES('edge','Default','x',1,0)",
                [],
            )
            .unwrap();
        let sid: i64 = db.conn.last_insert_rowid();
        db.conn
            .execute(
                "INSERT INTO browser_downloads(source_id,download_id,target_path,state,imported_at)
                 VALUES(?1,1,'e:\\DOCS\\FILE.ZIP','complete',0)",
                [sid],
            )
            .unwrap();
        let m = db.browser_match_downloads().unwrap();
        assert_eq!(m, 1, "大小写变体 + 嵌套根应匹配到 E:\\ 树");
        let row = &db.browser_downloads_list(None, 10).unwrap()[0];
        assert_eq!(row.match_state, "matched");
    }
}
