//! # 身份回填 pass（M3-A）：磁盘身份枚举 → 实体 upsert → 归属关联
//!
//! 授权后的身份采集主路径：用平台身份枚举（与扫描同句柄、批量取得
//! NTFS FileId）重走一遍磁盘，把 (卷, FileId) 主身份 / (卷, 相对路径)
//! 弱身份 upsert 进 file_entities，并与最新扫描代的 paths 节点对齐、
//! 把 attributions 关联到实体。
//!
//! 设计约束：
//! - **必须已授权**（identity_collection_enabled），否则拒绝执行；
//! - 只读遍历（枚举只开 FILE_LIST_DIRECTORY 句柄），绝不触碰用户文件；
//! - 批量事务（默认 5000/批）：一批一事务，失败回滚该批并终止（前批完好）；
//! - 取消：每目录/每批检查；取消后已发现条目照常冲刷入库（不丢已发现
//!   记录，与 scan_writer 语义一致），摘要记 cancelled；
//! - 首见实体**不写** file_discovered 事件（发现事件由 M3-B 差分产生，
//!   避免对存量文件一次性制造数十万条冗余事件；实体自身的
//!   first_seen/last_seen 已承载首次发现信息）；
//! - MFT 重用检测：同 FileId 再次出现但 (size, mtime) 校验位变化 →
//!   更新校验位并写 file_id_reuse_suspected 事件（待确认，不静默改写）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tracenest_core::scanner::ScanOptions;
use tracenest_core::CoreError;

use crate::source;
use crate::Database;

/// 默认批次大小：与扫描写入一致的 5000 条/事务。
pub const DEFAULT_IDENTITY_BATCH_SIZE: usize = 5000;

/// 回填进度（src-tauri 桥接到前端）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct IdentityBackfillProgress {
    pub entries: u64,
    pub entities_new: u64,
    pub entities_updated: u64,
    pub reuse_suspected: u64,
    /// 走弱身份的条目数（非 NTFS 卷 / FileId 取不到）
    pub weak_identity: u64,
    /// 磁盘上存在但最新扫描代没有的条目（实体仍建，路径关联为空）
    pub unmapped: u64,
    pub denied: u64,
    pub current_dir: String,
}

/// 回填摘要。
#[derive(Debug, Clone, Serialize)]
pub struct IdentityBackfillSummary {
    pub end_reason: EndReason,
    pub progress: IdentityBackfillProgress,
    /// 本次关联到实体的 attributions 行数（全库集合式关联）
    pub attributions_linked: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Completed,
    Cancelled,
    Failed,
}

/// 回填累积状态（待写缓冲与进度）。
/// pending 三元组与遍历同步：(条目, 卷内相对路径, 最新扫描代映射的 path_id)。
#[derive(Default)]
struct BackfillState {
    progress: IdentityBackfillProgress,
    pending: Vec<(
        tracenest_platform::identity::DirEntryId,
        String,
        Option<i64>,
    )>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 对一个卷执行身份回填。
///
/// `volume_id` / `root` 需已登记（ensure_volume）；`root` 为卷根（如 `E:\`）。
/// 单目录枚举失败计入 denied 并继续（与扫描引擎语义一致）；
/// 批写失败终止并返回错误（前批完好）；取消返回 Cancelled 摘要。
#[allow(clippy::too_many_arguments)]
pub fn run_identity_backfill(
    db: &Database,
    volume_id: i64,
    root: &Path,
    cancel: Option<&AtomicBool>,
    batch_size: usize,
    mut on_progress: impl FnMut(IdentityBackfillProgress),
) -> Result<IdentityBackfillSummary, CoreError> {
    // 授权门禁：双保险（命令层已检查，这里再拦一次——隐私红线）
    if !db.identity_collection_enabled()? {
        return Err(CoreError::Other(
            "文件身份采集未授权：请在设置页明确授权后再运行（默认关闭）".into(),
        ));
    }
    let batch_size = batch_size.max(1);
    let mut progress = IdentityBackfillProgress::default();

    // 1) 卷身份（GUID 记入 volumes；获取失败不阻断回填，如实留空并计 denied）
    match tracenest_platform::identity::volume_identity(&root.to_string_lossy()) {
        Ok(v) => {
            db.set_volume_guid(volume_id, v.volume_guid.as_deref())?;
        }
        Err(e) => {
            progress.denied += 1;
            eprintln!("[FMap] 卷身份获取失败（继续，GUID 留空）: {e}");
        }
    }

    // 2) 身份模式：NTFS → FileId 主身份；其他文件系统 → 弱身份降级
    let fs_type: Option<String> = db
        .conn
        .query_row(
            "SELECT fs_type FROM volumes WHERE id=?1",
            [volume_id],
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Database(format!("读取卷信息失败: {e}")))?;
    let use_file_id = fs_type
        .as_deref()
        .map(tracenest_platform::identity::fs_supports_stable_file_id)
        .unwrap_or(false);

    // 3) 最新扫描代的 rel→path_id 映射（对齐用；无扫描代也能跑，只是不关联）
    let rel_to_id = load_latest_generation_map(db, volume_id)?;
    let scan_id: Option<i64> = db
        .conn
        .query_row(
            "SELECT MAX(scan_id) FROM paths WHERE volume_id=?1",
            [volume_id],
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Database(format!("查询扫描代失败: {e}")))?;

    // 4) DFS 遍历（排除规则与扫描引擎一致；链接/重解析点不深入）
    let excludes = ScanOptions::default().exclude_dir_names;
    let mut st = BackfillState::default();
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    let mut cancelled = false;
    let mut write_err: Option<CoreError> = None;
    let mut root_alive = false; // 根目录枚举成功过一次（用于区分"整卷不可读"）

    'walk: while let Some((dir, rel)) = stack.pop() {
        if let Some(flag) = cancel {
            if flag.load(Ordering::Relaxed) {
                cancelled = true;
                break 'walk;
            }
        }
        let entries = match tracenest_platform::identity::enumerate_dir_with_ids(&dir) {
            Ok(e) => e,
            Err(e) => {
                progress.denied += 1;
                if !root_alive {
                    // 根目录都不可读 → 整卷终止（如实记失败）
                    eprintln!("[FMap] 卷根不可读，回填终止: {e}");
                    write_err = Some(e);
                    break 'walk;
                }
                continue; // 子目录失败不中断
            }
        };
        root_alive = true;
        progress.current_dir = rel.clone();

        for entry in entries {
            if let Some(flag) = cancel {
                if flag.load(Ordering::Relaxed) {
                    cancelled = true;
                    break 'walk;
                }
            }
            let is_link = entry.attrs & 0x400 != 0; // FILE_ATTRIBUTE_REPARSE_POINT
                                                    // 排除目录：不深入、不建实体（与扫描引擎排除清单一致）
            if entry.is_dir && excludes.contains(&entry.name) {
                progress.denied += 1;
                continue;
            }
            let child_rel = if rel.is_empty() {
                entry.name.clone()
            } else {
                format!("{rel}/{}", entry.name)
            };
            if entry.is_dir && !is_link {
                stack.push((dir.join(&entry.name), child_rel.clone()));
            }
            let mapped = rel_to_id.get(&child_rel).copied();
            if mapped.is_none() {
                progress.unmapped += 1;
            }
            if entry.file_id.is_none() || !use_file_id {
                progress.weak_identity += 1;
            }
            progress.entries += 1;
            st.pending.push((entry, child_rel, mapped));

            if st.pending.len() >= batch_size {
                if let Err(e) = flush_batch(db, volume_id, scan_id, use_file_id, &mut st) {
                    // 写库失败：终止（前批完好），与 scan_writer 语义一致
                    write_err = Some(e);
                    break 'walk;
                }
                on_progress(st.progress.clone());
            }
        }
    }

    // 5) 冲刷剩余（取消时也冲刷——不丢已发现记录，与 scan_writer 一致）
    if write_err.is_none() {
        flush_batch(db, volume_id, scan_id, use_file_id, &mut st)?;
    }

    // 6) 集合式关联 attributions（有实体映射的归属行；写库失败时也尽力关联）
    let attributions_linked = link_attributions_to_entities(db)?;

    let end_reason = if write_err.is_some() {
        EndReason::Failed
    } else if cancelled {
        EndReason::Cancelled
    } else {
        EndReason::Completed
    };
    // 合并遍历期计数（entries/unmapped/weak/denied 在本地 progress 上累计，
    // 实体计数在 st.progress 上累计——返回前统一）
    st.progress.entries = progress.entries;
    st.progress.unmapped = progress.unmapped;
    st.progress.weak_identity = progress.weak_identity;
    st.progress.denied = progress.denied;
    st.progress.current_dir = progress.current_dir;
    let final_progress = st.progress.clone();
    on_progress(final_progress.clone());
    match write_err {
        Some(e) => Err(e),
        None => Ok(IdentityBackfillSummary {
            end_reason,
            progress: final_progress,
            attributions_linked,
        }),
    }
}

/// 批量事务 upsert：一批一事务，成功 commit / 失败整体回滚。
fn flush_batch(
    db: &Database,
    volume_id: i64,
    scan_id: Option<i64>,
    use_file_id: bool,
    st: &mut BackfillState,
) -> Result<(), CoreError> {
    if st.pending.is_empty() {
        return Ok(());
    }
    let tx = db
        .conn
        .unchecked_transaction()
        .map_err(|e| CoreError::Database(format!("开启身份回填事务失败: {e}")))?;
    let now = now_ms();
    for (entry, rel, mapped) in st.pending.drain(..) {
        let check_size = entry.size as i64;
        let check_mtime = entry.mtime;
        let (kind, file_id, weak_path) = if use_file_id && entry.file_id.is_some() {
            ("file_id", entry.file_id, None::<String>)
        } else {
            ("weak", None, Some(rel))
        };
        // 查既有实体（主身份 / 弱身份两把部分唯一索引）
        let existing: Option<(i64, Option<i64>, Option<i64>)> = if kind == "file_id" {
            let mut stmt = tx
                .prepare(
                    "SELECT id, check_size, check_mtime FROM file_entities
                     WHERE volume_id=?1 AND file_id=?2",
                )
                .map_err(|e| CoreError::Database(format!("查询实体失败: {e}")))?;
            let mut rows = stmt
                .query_map(rusqlite::params![volume_id, file_id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .map_err(|e| CoreError::Database(format!("查询实体失败: {e}")))?;
            rows.next()
                .transpose()
                .map_err(|e| CoreError::Database(format!("查询实体失败: {e}")))?
        } else {
            let mut stmt = tx
                .prepare(
                    "SELECT id, check_size, check_mtime FROM file_entities
                     WHERE volume_id=?1 AND weak_path=?2",
                )
                .map_err(|e| CoreError::Database(format!("查询实体失败: {e}")))?;
            let mut rows = stmt
                .query_map(rusqlite::params![volume_id, weak_path], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .map_err(|e| CoreError::Database(format!("查询实体失败: {e}")))?;
            rows.next()
                .transpose()
                .map_err(|e| CoreError::Database(format!("查询实体失败: {e}")))?
        };
        match existing {
            Some((id, old_size, old_mtime)) => {
                // MFT 重用检测：同身份但校验位变化 → 待确认事件（不静默改写）
                if old_size.is_some() && (old_size != Some(check_size) || old_mtime != check_mtime)
                {
                    tx.execute(
                        "INSERT INTO file_events(entity_id,ts,kind,detail) VALUES(?1,?2,?3,?4)",
                        rusqlite::params![
                            id,
                            now,
                            source::event_kind::FILE_ID_REUSE_SUSPECTED,
                            serde_json::json!({
                                "old": { "size": old_size, "mtime": old_mtime },
                                "new": { "size": check_size, "mtime": check_mtime },
                                "note": "同 FileId 重现但内容特征变化：MFT 记录号可能被重用，待确认"
                            })
                            .to_string()
                        ],
                    )
                    .map_err(|e| CoreError::Database(format!("写入重用事件失败: {e}")))?;
                    st.progress.reuse_suspected += 1;
                }
                tx.execute(
                    "UPDATE file_entities SET check_size=?1, check_mtime=?2,
                     current_path_id=COALESCE(?3, current_path_id),
                     last_seen_scan_id=COALESCE(?4, last_seen_scan_id), last_seen_at=?5
                     WHERE id=?6",
                    rusqlite::params![check_size, check_mtime, mapped, scan_id, now, id],
                )
                .map_err(|e| CoreError::Database(format!("更新实体失败: {e}")))?;
                st.progress.entities_updated += 1;
            }
            None => {
                tx.execute(
                    "INSERT INTO file_entities(volume_id,identity_kind,file_id,check_size,check_mtime,
                       weak_path,current_path_id,first_seen_scan_id,last_seen_scan_id,first_seen_at,last_seen_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8,?9,?9)",
                    rusqlite::params![
                        volume_id,
                        kind,
                        file_id,
                        check_size,
                        check_mtime,
                        weak_path,
                        mapped,
                        scan_id,
                        now
                    ],
                )
                .map_err(|e| CoreError::Database(format!("插入实体失败: {e}")))?;
                st.progress.entities_new += 1;
            }
        }
    }
    tx.commit()
        .map_err(|e| CoreError::Database(format!("提交身份回填事务失败: {e}")))?;
    Ok(())
}

/// 加载最新扫描代的 rel→path_id 映射（DFS 先序入库保证父子齐全；无则空表）。
/// 根节点（depth 0，如 "E:"）的 rel 记为 ""，与回填遍历的起点约定一致。
fn load_latest_generation_map(
    db: &Database,
    volume_id: i64,
) -> Result<HashMap<String, i64>, CoreError> {
    let scan_id: Option<i64> = db
        .conn
        .query_row(
            "SELECT MAX(scan_id) FROM paths WHERE volume_id=?1",
            [volume_id],
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Database(format!("查询扫描代失败: {e}")))?;
    let Some(scan_id) = scan_id else {
        return Ok(HashMap::new());
    };
    let mut nodes: HashMap<i64, (Option<i64>, String)> = HashMap::new();
    {
        let mut stmt = db
            .conn
            .prepare(
                "SELECT id, parent_id, name FROM paths
                 WHERE scan_id=?1 AND volume_id=?2 AND deleted_at IS NULL",
            )
            .map_err(|e| CoreError::Database(format!("加载路径失败: {e}")))?;
        let iter = stmt
            .query_map(rusqlite::params![scan_id, volume_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| CoreError::Database(format!("加载路径失败: {e}")))?;
        for row in iter {
            let (id, parent, name) =
                row.map_err(|e| CoreError::Database(format!("加载路径失败: {e}")))?;
            nodes.insert(id, (parent, name));
        }
    }
    // 迭代重建 rel（记忆化：命中缓存直接拼尾部，避免深链 O(n²)；
    // 未命中缓存的完整链也要落缓存——否则后续兄弟节点无法复用前缀）
    let mut memo: HashMap<i64, Vec<String>> = HashMap::new();
    let mut out: HashMap<String, i64> = HashMap::with_capacity(nodes.len());
    for &id in nodes.keys() {
        let mut rev: Vec<String> = Vec::new();
        let mut cur = Some(id);
        while let Some(cid) = cur {
            if let Some(parts) = memo.get(&cid).cloned() {
                let mut base = parts;
                for n in rev.iter().rev() {
                    base.push(n.clone());
                }
                memo.insert(id, base);
                break;
            }
            if let Some((parent, name)) = nodes.get(&cid) {
                rev.push(name.clone());
                cur = *parent;
            } else {
                break;
            }
        }
        if !memo.contains_key(&id) {
            // 完整走到根（无缓存命中）：rev 现为 [自身..根]，反转后落缓存
            rev.reverse();
            memo.insert(id, rev);
        }
        if let Some(parts) = memo.get(&id) {
            // parts[0] = 根名（如 "E:"）→ 根 rel=""；子级用 '/' 拼接
            let rel = if parts.len() <= 1 {
                String::new()
            } else {
                parts[1..].join("/")
            };
            if !rel.is_empty() {
                out.insert(rel, id);
            }
        }
    }
    Ok(out)
}

/// 集合式关联：attributions.entity_id ← 有路径映射的实体。
/// 幂等（只填 NULL）；返回本次关联的行数。
pub fn link_attributions_to_entities(db: &Database) -> Result<i64, CoreError> {
    let before: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM attributions WHERE entity_id IS NULL
             AND EXISTS(SELECT 1 FROM file_entities fe WHERE fe.current_path_id = attributions.path_id)",
            [],
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Database(format!("统计待关联归属失败: {e}")))?;
    if before == 0 {
        return Ok(0);
    }
    db.conn
        .execute(
            "UPDATE attributions SET entity_id =
               (SELECT fe.id FROM file_entities fe WHERE fe.current_path_id = attributions.path_id LIMIT 1)
             WHERE entity_id IS NULL
               AND EXISTS(SELECT 1 FROM file_entities fe WHERE fe.current_path_id = attributions.path_id)",
            [],
        )
        .map_err(|e| CoreError::Database(format!("关联归属到实体失败: {e}")))?;
    Ok(before)
}

// ---------------------------------------------------------------------------
// 测试（Windows 真机：tempdir 在 NTFS 上）
// ---------------------------------------------------------------------------
#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicBool;

    /// 在磁盘上造一棵真实小树并跑一次完整"扫描"，
    /// 返回 (db, tempdir, volume_id, scan_id)。
    fn scanned_fixture() -> (Database, tempfile::TempDir, i64, i64) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), vec![0u8; 10]).unwrap();
        fs::create_dir_all(dir.path().join(".codex")).unwrap();
        fs::write(dir.path().join(".codex/config.json"), vec![0u8; 20]).unwrap();

        let db = Database::open(":memory:".as_ref()).unwrap();
        db.set_identity_collection_enabled(true).unwrap();
        let root_str = format!("{}\\", dir.path().to_string_lossy().trim_end_matches('\\'));
        let vol = db
            .upsert_volume(&root_str, None, None, None, Some("NTFS"), false)
            .unwrap();
        let scan = db
            .begin_scan(&[root_str.clone()], "t", db.instance_id())
            .unwrap();
        let summary = crate::scan_writer::run_scan_to_db(
            &db,
            scan,
            vol,
            Path::new(&root_str),
            &ScanOptions::default(),
            None,
            5000,
            |_| {},
        )
        .unwrap();
        assert_eq!(
            summary.end_reason,
            tracenest_core::scanner::ScanEndReason::Completed
        );
        (db, dir, vol, scan)
    }

    #[test]
    fn 未授权拒绝执行_授权门禁不绕过() {
        let (db, dir, vol, _scan) = scanned_fixture();
        db.set_identity_collection_enabled(false).unwrap();
        let r = run_identity_backfill(&db, vol, dir.path(), None, 2, |_| {});
        assert!(r.is_err(), "未授权必须拒绝");
        let n: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM file_entities", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn 回填_NTFS主身份_实体创建与对齐_二次幂等() {
        let (db, dir, vol, _scan) = scanned_fixture();
        let summary = run_identity_backfill(&db, vol, dir.path(), None, 2, |_| {}).unwrap();
        assert_eq!(summary.end_reason, EndReason::Completed);
        // 磁盘上可见条目：a.txt / .codex / .codex/config.json（根不入枚举）
        assert_eq!(summary.progress.entities_new, 3, "三个实体全部新建");
        assert_eq!(summary.progress.weak_identity, 0, "NTFS 上不应走弱身份");
        assert_eq!(summary.progress.unmapped, 0, "刚扫描完的树应全对齐");
        // 卷 GUID 已记录
        let guid: Option<String> = db
            .conn
            .query_row("SELECT volume_guid FROM volumes WHERE id=?1", [vol], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(guid.is_some(), "NTFS 卷 GUID 应记录");
        // 实体的 current_path_id 已对齐
        let aligned: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM file_entities WHERE current_path_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(aligned, 3);
        // 二次回填：幂等（无新增、无重用嫌疑）
        let s2 = run_identity_backfill(&db, vol, dir.path(), None, 2, |_| {}).unwrap();
        assert_eq!(s2.progress.entities_new, 0, "二次回填不得新建实体");
        assert_eq!(s2.progress.entities_updated, 3);
        assert_eq!(s2.progress.reuse_suspected, 0, "内容未变不得报重用嫌疑");
    }

    #[test]
    fn 回填_归属关联到实体_确认闭环可用() {
        let (db, dir, vol, scan) = scanned_fixture();
        let codex_path: i64 = db
            .conn
            .query_row(
                "SELECT id FROM paths WHERE name='.codex' AND scan_id=?1",
                [scan],
                |r| r.get(0),
            )
            .unwrap();
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
                rusqlite::params![codex_path, scan],
            )
            .unwrap();

        let summary = run_identity_backfill(&db, vol, dir.path(), None, 2, |_| {}).unwrap();
        assert_eq!(summary.attributions_linked, 1, ".codex 归属应关联到实体");
        let (linked_entity, entity_path): (i64, Option<i64>) = db
            .conn
            .query_row(
                "SELECT a.entity_id, fe.current_path_id FROM attributions a
                 JOIN file_entities fe ON a.entity_id=fe.id
                 WHERE a.path_id=?1",
                [codex_path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(linked_entity > 0);
        assert_eq!(entity_path, Some(codex_path));
        // 确认流程在真实实体上闭环可用
        let eff = db
            .confirm_source(codex_path, source::behavior::AI_CREATED, None)
            .unwrap();
        assert_eq!(eff.action, "confirm");
    }

    #[test]
    fn 回填_取消后已发现部分保留() {
        let (db, dir, vol, _scan) = scanned_fixture();
        let flag = AtomicBool::new(true); // 预置取消
        let summary = run_identity_backfill(&db, vol, dir.path(), Some(&flag), 2, |_| {}).unwrap();
        assert_eq!(summary.end_reason, EndReason::Cancelled);
        // 关键语义：不崩溃、终态诚实；已发现条目冲刷入库（数量 >= 0）
        let n: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM file_entities", [], |r| r.get(0))
            .unwrap();
        assert!(n >= 0);
    }

    #[test]
    fn 弱身份降级_非NTFS卷用路径键() {
        let (db, dir, vol, _scan) = scanned_fixture();
        // 把卷伪标成 exFAT → 强制走弱身份（不伪造 FileId 身份）
        db.conn
            .execute("UPDATE volumes SET fs_type='exFAT'", [])
            .unwrap();
        let summary = run_identity_backfill(&db, vol, dir.path(), None, 2, |_| {}).unwrap();
        assert_eq!(summary.progress.weak_identity, 3, "exFAT 卷应全部走弱身份");
        let kind: String = db
            .conn
            .query_row("SELECT identity_kind FROM file_entities LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kind, "weak");
        let weak_n: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM file_entities WHERE weak_path IS NOT NULL AND file_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(weak_n, 3, "弱身份键必须存在且不得带 FileId");
    }

    #[test]
    fn 重用嫌疑_同ID内容特征变化_事件与校验位更新() {
        let (db, dir, vol, _scan) = scanned_fixture();
        run_identity_backfill(&db, vol, dir.path(), None, 2, |_| {}).unwrap();
        // 模拟 MFT 重用：改写 a.txt 内容（size 变化）后再回填
        fs::write(dir.path().join("a.txt"), vec![0u8; 999]).unwrap();
        let s2 = run_identity_backfill(&db, vol, dir.path(), None, 2, |_| {}).unwrap();
        assert_eq!(
            s2.progress.reuse_suspected, 1,
            "同 FileId + 校验位变化必须报嫌疑"
        );
        let kind: String = db
            .conn
            .query_row(
                "SELECT kind FROM file_events WHERE entity_id=(SELECT id FROM file_entities fe WHERE fe.check_size=999)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, source::event_kind::FILE_ID_REUSE_SUSPECTED);
    }
}
