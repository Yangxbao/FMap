//! # M4 浏览查询层：磁盘地图 + 文件浏览的真实数据源
//!
//! 语义边界：
//! - 全部基于**已有扫描数据库**（paths/scans/volumes/attributions/file_events）；
//! - 不触碰用户文件系统（浏览的是"上次扫描快照"，UI 必须标注采集时间与覆盖状态）；
//! - 未扫描/无扫描代 → 明确的"未扫描"状态，绝不显示成空目录；
//! - 覆盖缺口（coverage_gaps）随卷概览返回——缺口区域不得当成完整；
//! - 目录 size 为扫描时的子树聚合值（历史快照，非实时）。

use serde::Serialize;
use tracenest_core::CoreError;

use crate::Database;

/// 卷概览行（磁盘地图页数据源）。
#[derive(Debug, Clone, Serialize)]
pub struct VolumeOverview {
    pub volume_id: i64,
    pub root: String,
    pub label: Option<String>,
    pub total_bytes: Option<i64>,
    pub free_bytes: Option<i64>,
    pub fs_type: Option<String>,
    /// 该卷最新扫描代（无扫描 = None → UI 显示"未扫描"）
    pub latest_scan_id: Option<i64>,
    pub latest_scan_status: Option<String>,
    pub latest_scan_finished_at: Option<i64>,
    /// 最新扫描代中的路径计数（文件/目录）
    pub files: i64,
    pub dirs: i64,
    /// 覆盖缺口条数（denied/failed/excluded——未覆盖区域不得当作完整）
    pub coverage_gap_count: i64,
    /// 扫描根节点 path_id（磁盘地图下钻入口；无扫描 None）
    pub root_path_id: Option<i64>,
}

/// 目录子项行。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseChild {
    pub path_id: i64,
    pub name: String,
    pub is_dir: bool,
    /// 子树聚合（目录）/文件大小（字节；扫描时快照）
    pub size: i64,
    pub mtime: Option<i64>,
}

/// 面包屑节点。
#[derive(Debug, Clone, Serialize)]
pub struct Crumb {
    pub path_id: i64,
    pub name: String,
    pub is_root: bool,
}

/// 搜索结果行（FTS；定位到最新扫描代）。
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub path_id: i64,
    pub scan_id: i64,
    pub volume_id: i64,
    pub volume_root: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub mtime: Option<i64>,
    /// 绝对路径（快照语义；显示用）
    pub abs_path: String,
}

/// 路径详情（文件浏览行详情抽屉）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowsePathDetail {
    pub path_id: i64,
    pub scan_id: i64,
    pub volume_id: i64,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub mtime: Option<i64>,
    pub ctime: Option<i64>,
    pub abs_path: String,
    /// M2 归属（该路径行上的全部工具结论）
    pub attributions: Vec<AttrBrief>,
    /// 观察事件（该路径挂点的差分事件）
    pub events: Vec<EventBrief>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttrBrief {
    pub tool_key: String,
    pub tool_name: String,
    pub confidence: String,
    pub source_behavior: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventBrief {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
}

impl Database {
    /// 全部卷的概览（含最新扫描状态与覆盖缺口计数）。
    pub fn volume_overviews(&self) -> Result<Vec<VolumeOverview>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT v.id, v.root, v.label, v.total_bytes, v.free_bytes, v.fs_type,
                        (SELECT MAX(scan_id) FROM paths p WHERE p.volume_id = v.id)
                 FROM volumes v ORDER BY v.root",
            )
            .map_err(|e| CoreError::Database(format!("卷概览查询失败: {e}")))?;
        let it = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                ))
            })
            .map_err(|e| CoreError::Database(format!("卷概览查询失败: {e}")))?;
        let mut out = Vec::new();
        for row in it {
            let (vid, root, label, total, free, fs, latest) =
                row.map_err(|e| CoreError::Database(format!("卷概览读取失败: {e}")))?;
            let (status, finished): (Option<String>, Option<i64>) = match latest {
                Some(sid) => self
                    .conn
                    .query_row(
                        "SELECT status, finished_at FROM scans WHERE id=?1",
                        [sid],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map_err(|e| CoreError::Database(format!("扫描状态读取失败: {e}")))?,
                None => (None, None),
            };
            let (files, dirs) = match latest {
                Some(sid) => self
                    .conn
                    .query_row(
                        "SELECT COALESCE(SUM(CASE WHEN is_dir=0 THEN 1 ELSE 0 END),0),
                                COALESCE(SUM(CASE WHEN is_dir=1 THEN 1 ELSE 0 END),0)
                         FROM paths WHERE scan_id=?1 AND deleted_at IS NULL",
                        [sid],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map_err(|e| CoreError::Database(format!("路径计数失败: {e}")))?,
                None => (0, 0),
            };
            let gap_count: i64 = match latest {
                Some(sid) => self
                    .conn
                    .query_row(
                        "SELECT COALESCE((SELECT json_array_length(coverage_gaps,'$.paths') FROM scans WHERE id=?1),0)",
                        [sid],
                        |r| r.get(0),
                    )
                    .map_err(|e| CoreError::Database(format!("缺口计数失败: {e}")))?,
                None => 0,
            };
            let root_path_id: Option<i64> = match latest {
                Some(sid) => self
                    .conn
                    .query_row(
                        "SELECT id FROM paths WHERE scan_id=?1 AND depth=0 AND parent_id IS NULL LIMIT 1",
                        [sid],
                        |r| r.get(0),
                    )
                    .ok(),
                None => None,
            };
            out.push(VolumeOverview {
                volume_id: vid,
                root,
                label,
                total_bytes: total,
                free_bytes: free,
                fs_type: fs,
                latest_scan_id: latest,
                latest_scan_status: status,
                latest_scan_finished_at: finished,
                files,
                dirs,
                coverage_gap_count: gap_count,
                root_path_id,
            });
        }
        Ok(out)
    }

    /// 目录子项（按 is_dir 后大小排序；目录含 size 聚合快照）。
    pub fn browse_children(&self, parent_id: i64) -> Result<Vec<BrowseChild>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, is_dir, size, mtime FROM paths
                 WHERE parent_id=?1 AND deleted_at IS NULL
                 ORDER BY is_dir DESC, size DESC, name ASC LIMIT 2000",
            )
            .map_err(|e| CoreError::Database(format!("目录浏览查询失败: {e}")))?;
        let it = stmt
            .query_map([parent_id], |r| {
                Ok(BrowseChild {
                    path_id: r.get(0)?,
                    name: r.get(1)?,
                    is_dir: r.get::<_, i64>(2)? != 0,
                    size: r.get(3)?,
                    mtime: r.get(4)?,
                })
            })
            .map_err(|e| CoreError::Database(format!("目录浏览查询失败: {e}")))?;
        let mut out = Vec::new();
        for r in it {
            out.push(r.map_err(|e| CoreError::Database(format!("目录浏览读取失败: {e}")))?);
        }
        Ok(out)
    }

    /// 面包屑（根 → 当前；含自身）。
    pub fn browse_breadcrumb(&self, path_id: i64) -> Result<Vec<Crumb>, CoreError> {
        let mut chain: Vec<(i64, String, bool)> = Vec::new();
        let mut cur = Some(path_id);
        while let Some(pid) = cur {
            let (parent, name): (Option<i64>, String) = self
                .conn
                .query_row(
                    "SELECT parent_id, name FROM paths WHERE id=?1",
                    [pid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| CoreError::Database(format!("面包屑查询失败: {e}")))?;
            chain.push((pid, name, parent.is_none()));
            cur = parent;
        }
        chain.reverse();
        Ok(chain
            .into_iter()
            .map(|(id, name, is_root)| Crumb {
                path_id: id,
                name,
                is_root,
            })
            .collect())
    }

    /// 批量构建绝对路径（搜索结果 N+1 优化）：迭代加载 id 集合的全部祖先
    /// 层进内存，再逐条建链——查询次数从 O(N×深度) 降为 O(祖先层数)。
    /// 语义与 [`Self::abs_path_of`] 一致（快照：卷根 + 名称链，根节点名不入链）。
    pub fn abs_paths_bulk(
        &self,
        ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, String>, CoreError> {
        use std::collections::HashMap;
        // 1) 迭代加载节点层（id → (parent, volume, name)）
        let mut nodes: HashMap<i64, (Option<i64>, Option<i64>, String)> = HashMap::new();
        let mut frontier: Vec<i64> = ids.to_vec();
        while !frontier.is_empty() {
            let missing: Vec<i64> = frontier
                .iter()
                .filter(|id| !nodes.contains_key(id))
                .copied()
                .collect();
            if missing.is_empty() {
                break;
            }
            let mut next_parents: Vec<i64> = Vec::new();
            for chunk in missing.chunks(400) {
                let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let sql = format!(
                    "SELECT id, parent_id, volume_id, name FROM paths WHERE id IN ({placeholders})"
                );
                let mut stmt = self
                    .conn
                    .prepare(&sql)
                    .map_err(|e| CoreError::Database(format!("加载路径节点失败: {e}")))?;
                let it = stmt
                    .query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, Option<i64>>(1)?,
                            r.get::<_, Option<i64>>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    })
                    .map_err(|e| CoreError::Database(format!("加载路径节点失败: {e}")))?;
                for row in it {
                    let (id, parent, volume, name) =
                        row.map_err(|e| CoreError::Database(format!("加载路径节点失败: {e}")))?;
                    nodes.insert(id, (parent, volume, name));
                    if let Some(p) = parent {
                        next_parents.push(p);
                    }
                }
            }
            frontier = next_parents;
        }
        // 2) 卷根映射
        let mut volume_roots: HashMap<i64, String> = HashMap::new();
        for (_, vid, _) in nodes.values() {
            if let Some(vid) = vid {
                if !volume_roots.contains_key(vid) {
                    if let Ok((id, root)) = self.conn.query_row(
                        "SELECT id, root FROM volumes WHERE id=?1",
                        [vid],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
                    ) {
                        volume_roots.insert(id, root);
                    }
                }
            }
        }
        // 3) 逐 id 建链（内存遍历；根节点名不入链；仅对实际存在的 id 产出）
        let mut out: HashMap<i64, String> = HashMap::new();
        for &id in ids {
            if !nodes.contains_key(&id) {
                continue; // 不存在的 id 不产条目（与 abs_path_of 报错语义对齐）
            }
            let mut names: Vec<String> = Vec::new();
            let mut root = String::new();
            let mut cur = Some(id);
            while let Some(pid) = cur {
                match nodes.get(&pid) {
                    Some((parent, vid, name)) => {
                        names.push(name.clone());
                        if let Some(vid) = vid {
                            root = volume_roots.get(vid).cloned().unwrap_or_default();
                            if parent.is_none() {
                                names.pop(); // 卷根节点名不进入路径
                                break;
                            }
                        }
                        cur = *parent;
                    }
                    None => break,
                }
            }
            names.reverse();
            let joined = names.join("\\");
            out.insert(
                id,
                if joined.is_empty() {
                    root
                } else {
                    format!("{root}{joined}")
                },
            );
        }
        Ok(out)
    }

    /// 由路径行构建绝对路径（快照语义：卷根 + 名称链）。
    pub fn abs_path_of(&self, path_id: i64) -> Result<String, CoreError> {
        let mut names: Vec<String> = Vec::new();
        let mut cur = Some(path_id);
        let mut root = String::new();
        while let Some(pid) = cur {
            let row: (Option<i64>, Option<i64>, String) = self
                .conn
                .query_row(
                    "SELECT parent_id, volume_id, name FROM paths WHERE id=?1",
                    [pid],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(|e| CoreError::Database(format!("路径构建失败: {e}")))?;
            names.push(row.2);
            if let Some(vid) = row.1 {
                root = self
                    .conn
                    .query_row("SELECT root FROM volumes WHERE id=?1", [vid], |r| r.get(0))
                    .map_err(|e| CoreError::Database(format!("路径构建失败: {e}")))?;
                // 卷根节点（depth 0）以上不再向上
                let is_root_node: bool = self
                    .conn
                    .query_row(
                        "SELECT CASE WHEN parent_id IS NULL THEN 1 ELSE 0 END FROM paths WHERE id=?1",
                        [pid],
                        |r| r.get::<_, i64>(0),
                    )
                    .map(|v| v != 0)
                    .unwrap_or(false);
                if is_root_node {
                    names.pop(); // 卷根节点名不进入路径（root 已含盘符）
                    break;
                }
            }
            cur = row.0;
        }
        names.reverse();
        let joined = names.join("\\");
        if joined.is_empty() {
            Ok(root)
        } else {
            Ok(format!("{root}{joined}"))
        }
    }

    /// FTS 文件名搜索（定位到各卷最新扫描代；前缀匹配）。
    ///
    /// 性能设计（真实库 330 万行实测教训）：
    /// - 「最新代」过滤用**预计算的 IN 列表**，绝不用逐行相关子查询
    ///   （`p.scan_id = (SELECT MAX...)` 曾把 CJK 兜底拖到 15 秒）；
    /// - 单字符查询直接走 LIKE（FTS 前缀 "a"* 需展开整本词典，曾达 15 秒）；
    /// - FTS 命中集内层先截断（LIMIT ×4 候选池），外层 ORDER BY 不短路也能保证响应上限；
    /// - FTS 对连续 CJK 失配（unicode61 分词）→ 零命中时回退 LIKE。
    pub fn search_paths(&self, query: &str, limit: i64) -> Result<Vec<SearchHit>, CoreError> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        // FTS 前缀语法：词尾加 *；转义 FTS 特殊字符（只保留字母数字与 CJK）
        let sanitized: String = q
            .chars()
            .filter(|c| c.is_alphanumeric() || !c.is_ascii())
            .collect();
        if sanitized.is_empty() {
            return Ok(vec![]);
        }
        let lim = limit.clamp(1, 200);
        // 各卷最新代 IN 列表（卷数个位数；数值来自 DB 整型列，无注入面）
        let latest_scans: Vec<i64> = {
            let mut stmt = self
                .conn
                .prepare("SELECT MAX(scan_id) FROM paths GROUP BY volume_id")
                .map_err(|e| CoreError::Database(format!("搜索失败: {e}")))?;
            let it = stmt
                .query_map([], |r| r.get::<_, Option<i64>>(0))
                .map_err(|e| CoreError::Database(format!("搜索失败: {e}")))?;
            let mut v = Vec::new();
            for r in it {
                if let Some(id) = r.map_err(|e| CoreError::Database(format!("搜索失败: {e}")))?
                {
                    v.push(id);
                }
            }
            v
        };
        if latest_scans.is_empty() {
            return Ok(vec![]);
        }
        let in_list = latest_scans
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let base_where = format!("p.scan_id IN ({in_list}) AND p.deleted_at IS NULL");
        type SearchRawRow = (i64, i64, i64, String, String, i64, i64, Option<i64>);
        let mut raw: Vec<SearchRawRow> = Vec::new();
        // 单字符查询：FTS 前缀展开代价过高（词典级），直接 LIKE
        if sanitized.chars().count() >= 2 {
            let fts_query = format!("\"{sanitized}\"*");
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT p.id, p.scan_id, p.volume_id, v.root, p.name, p.is_dir, p.size, p.mtime
                     FROM (
                       SELECT rowid FROM paths_fts WHERE paths_fts MATCH ?1 LIMIT ?2
                     ) f
                     JOIN paths p ON p.id = f.rowid
                     JOIN volumes v ON v.id = p.volume_id
                     WHERE {base_where}
                     ORDER BY p.size DESC LIMIT ?2",
                ))
                .map_err(|e| CoreError::Database(format!("搜索失败: {e}")))?;
            let it = stmt
                .query_map(rusqlite::params![fts_query, lim], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, Option<i64>>(7)?,
                    ))
                })
                .map_err(|e| CoreError::Database(format!("搜索失败: {e}")))?;
            for row in it {
                raw.push(row.map_err(|e| CoreError::Database(format!("搜索读取失败: {e}")))?);
            }
        }
        // LIKE 兜底/直查：FTS 零命中（CJK 失配）或单字符查询
        if raw.is_empty() {
            let like = format!("%{}%", sanitized.replace(['%', '_'], ""));
            let mut stmt2 = self
                .conn
                .prepare(&format!(
                    "SELECT p.id, p.scan_id, p.volume_id, v.root, p.name, p.is_dir, p.size, p.mtime
                     FROM paths p JOIN volumes v ON v.id = p.volume_id
                     WHERE p.name LIKE ?1 AND {base_where}
                     ORDER BY p.size DESC LIMIT ?2",
                ))
                .map_err(|e| CoreError::Database(format!("搜索失败: {e}")))?;
            let it2 = stmt2
                .query_map(rusqlite::params![like, lim], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, Option<i64>>(7)?,
                    ))
                })
                .map_err(|e| CoreError::Database(format!("搜索失败: {e}")))?;
            for row in it2 {
                raw.push(row.map_err(|e| CoreError::Database(format!("搜索读取失败: {e}")))?);
            }
        }
        let ids: Vec<i64> = raw.iter().map(|r| r.0).collect();
        let abs_map = self.abs_paths_bulk(&ids).unwrap_or_default();
        let mut out = Vec::new();
        for (pid, scan_id, vid, vroot, name, is_dir, size, mtime) in raw {
            let abs = abs_map.get(&pid).cloned().unwrap_or_else(|| vroot.clone());
            out.push(SearchHit {
                path_id: pid,
                scan_id,
                volume_id: vid,
                volume_root: vroot,
                name,
                is_dir: is_dir != 0,
                size,
                mtime,
                abs_path: abs,
            });
        }
        Ok(out)
    }

    /// 路径详情（归属 + 事件；详情抽屉数据源）。
    pub fn browse_path_detail(&self, path_id: i64) -> Result<BrowsePathDetail, CoreError> {
        let (scan_id, volume_id, name, is_dir, size, mtime, ctime): (
            i64,
            i64,
            String,
            i64,
            i64,
            Option<i64>,
            Option<i64>,
        ) = self
            .conn
            .query_row(
                "SELECT scan_id, volume_id, name, is_dir, size, mtime, ctime FROM paths WHERE id=?1",
                [path_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .map_err(|_| CoreError::Other(format!("路径不存在: {path_id}")))?;
        let abs = self.abs_path_of(path_id)?;
        let mut attributions = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT t.key, t.name, a.confidence, a.source_behavior
                     FROM attributions a JOIN tools t ON t.id = a.tool_id
                     WHERE a.path_id=?1 ORDER BY a.score DESC",
                )
                .map_err(|e| CoreError::Database(format!("归属查询失败: {e}")))?;
            let it = stmt
                .query_map([path_id], |r| {
                    Ok(AttrBrief {
                        tool_key: r.get(0)?,
                        tool_name: r.get(1)?,
                        confidence: r.get(2)?,
                        source_behavior: r.get(3)?,
                    })
                })
                .map_err(|e| CoreError::Database(format!("归属查询失败: {e}")))?;
            for r in it {
                attributions.push(r.map_err(|e| CoreError::Database(format!("{e}")))?);
            }
        }
        let mut events = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, ts, kind FROM file_events WHERE path_id=?1 ORDER BY ts DESC, id DESC LIMIT 50",
                )
                .map_err(|e| CoreError::Database(format!("事件查询失败: {e}")))?;
            let it = stmt
                .query_map([path_id], |r| {
                    Ok(EventBrief {
                        id: r.get(0)?,
                        ts: r.get(1)?,
                        kind: r.get(2)?,
                    })
                })
                .map_err(|e| CoreError::Database(format!("事件查询失败: {e}")))?;
            for r in it {
                events.push(r.map_err(|e| CoreError::Database(format!("{e}")))?);
            }
        }
        Ok(BrowsePathDetail {
            path_id,
            scan_id,
            volume_id,
            name,
            is_dir: is_dir != 0,
            size,
            mtime,
            ctime,
            abs_path: abs,
            attributions,
            events,
        })
    }
}

// ---------------------------------------------------------------------------
// 测试（纯 DB 种子；快照语义 / 未扫描状态 / FTS / 详情）
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use tracenest_core::scan_scope::ScanScope;

    fn mem_db() -> Database {
        Database::open(":memory:".as_ref()).expect("内存库应能打开")
    }

    fn seed(db: &Database) -> (i64, i64, i64, i64) {
        let vol = db
            .upsert_volume(
                "E:\\",
                Some("数据"),
                Some(1000),
                Some(400),
                Some("NTFS"),
                false,
            )
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
        db.set_scan_coverage(
            scan,
            &tracenest_core::scan_scope::CoverageGaps {
                paths: vec![tracenest_core::scan_scope::CoverageGap {
                    rel_path: "locked".into(),
                    kind: "dir".into(),
                    reason: "denied".into(),
                }],
                truncated: false,
            },
        )
        .unwrap();
        let root = db
            .insert_path(scan, vol, None, "E:", 0, true, 1000, Some(0), 0)
            .unwrap();
        let proj = db
            .insert_path(scan, vol, Some(root), "PROJ", 1, true, 700, Some(0), 0)
            .unwrap();
        let doc = db
            .insert_path(
                scan,
                vol,
                Some(root),
                "报告.docx",
                1,
                false,
                300,
                Some(5),
                0,
            )
            .unwrap();
        db.insert_path(scan, vol, Some(proj), "main.rs", 2, false, 700, Some(6), 0)
            .unwrap();
        db.finish_scan(scan, "done", 2, 2, 0, None).unwrap();
        // FTS 索引（真实扫描由 scan_writer 收口时重建；种子手工镜像）
        db.fts_rebuild_for_volume(vol, scan).unwrap();
        (vol, scan, root, doc)
    }

    #[test]
    fn 卷概览_最新代状态_计数_缺口_未扫描诚实() {
        let db = mem_db();
        // 无扫描的卷 → 未扫描状态
        db.upsert_volume("X:\\", None, None, None, None, false)
            .unwrap();
        seed(&db);
        let ovs = db.volume_overviews().unwrap();
        assert_eq!(ovs.len(), 2);
        let x = ovs.iter().find(|o| o.root.starts_with("X:")).unwrap();
        assert!(x.latest_scan_id.is_none(), "无扫描 → 未扫描");
        assert_eq!((x.files, x.dirs), (0, 0));
        let e = ovs.iter().find(|o| o.root.starts_with("E:")).unwrap();
        assert_eq!(e.latest_scan_status.as_deref(), Some("done"));
        assert_eq!((e.files, e.dirs), (2, 2));
        assert_eq!(e.coverage_gap_count, 1, "覆盖缺口计数");
        assert!(e.root_path_id.is_some());
        assert_eq!(e.total_bytes, Some(1000));
    }

    #[test]
    fn 目录浏览_面包屑_绝对路径() {
        let db = mem_db();
        let (_vol, _scan, root, doc) = seed(&db);
        let kids = db.browse_children(root).unwrap();
        assert_eq!(kids.len(), 2, "根下 PROJ + 报告.docx");
        assert!(kids[0].is_dir, "目录优先");
        // 绝对路径
        let abs = db.abs_path_of(doc).unwrap();
        assert_eq!(abs, "E:\\报告.docx", "卷根节点名不重复进入路径");
        // 面包屑
        let crumbs = db.browse_breadcrumb(doc).unwrap();
        assert_eq!(crumbs.len(), 2);
        assert!(crumbs[0].is_root);
        assert_eq!(crumbs[1].name, "报告.docx");
    }

    #[test]
    fn 批量绝对路径_与单个一致_根节点与深层混合() {
        let db = mem_db();
        let (_vol, _scan, root, doc) = seed(&db);
        // PROJ 的 id（根下第一个目录）
        let proj = db.browse_children(root).unwrap()[0].path_id;
        let map = db.abs_paths_bulk(&[doc, root, proj, 999999]).unwrap();
        assert_eq!(map.get(&doc).unwrap(), "E:\\报告.docx");
        assert_eq!(map.get(&root).unwrap(), "E:\\", "根节点 → 纯卷根");
        assert_eq!(map.get(&proj).unwrap(), "E:\\PROJ");
        assert!(!map.contains_key(&999999), "不存在的 id 不产条目");
        // 与单个版语义一致
        assert_eq!(map.get(&doc).unwrap(), &db.abs_path_of(doc).unwrap());
    }

    #[test]
    fn fts搜索_定位最新代_中文_返回绝对路径() {
        let db = mem_db();
        let (_vol, _scan, _root, doc) = seed(&db);
        let hits = db.search_paths("报告", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path_id, doc);
        assert_eq!(hits[0].abs_path, "E:\\报告.docx");
        // 空查询 → 空
        assert!(db.search_paths("", 10).unwrap().is_empty());
        // 纯符号 → 空（不崩溃）
        assert!(db.search_paths("!*", 10).unwrap().is_empty());
    }

    #[test]
    fn 路径详情_归属与事件_不存在报错() {
        let db = mem_db();
        let (_vol, scan, _root, doc) = seed(&db);
        db.conn
            .execute(
                "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES('codex','Codex','','cli','#111',1)",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior)
                 VALUES(?1,?2,1,'verified',1.0,'{}','unknown')",
                rusqlite::params![doc, scan],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,scan_id,detail)
                 VALUES(?1,1,'报告.docx','报告.DOCX',0,'file_modified',?2,'{}')",
                rusqlite::params![doc, scan],
            )
            .unwrap();
        let d = db.browse_path_detail(doc).unwrap();
        assert_eq!(d.abs_path, "E:\\报告.docx");
        assert_eq!(d.attributions.len(), 1);
        assert_eq!(d.attributions[0].tool_key, "codex");
        assert_eq!(d.events.len(), 1);
        assert!(db.browse_path_detail(999999).is_err(), "不存在路径报错");
    }
}
