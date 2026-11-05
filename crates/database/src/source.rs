//! # 来源模型（M3-A）：语义常量 + 授权开关 + 用户确认 / 撤销 / 时间线 API
//!
//! 三层分模型（M2 交接文档 §5/§17 审查终版，**不得推翻**）：
//! 1. **原始事件与证据**（file_events / source_evidence）：append-only，
//!    一经写入不可被自动判断覆盖；
//! 2. **自动候选**（source_candidates）：规则/证据更新后按 generation
//!    整体 DELETE+重算，绝不触碰事件与确认；
//! 3. **用户确认**（source_confirmations）：append-only 保留修正历史，
//!    最高可信，覆盖候选展示。
//!
//! 语义红线：
//! - 工具关联 ≠ 文件来源（attributions 原样保留，不转换为来源行为）；
//! - M2/M3-A 的自动分析**恒不产出** AI_CREATED/AI_MODIFIED——本模块
//!   提供的唯一来源行为写入路径是用户确认；
//! - UNKNOWN 永久合法：宁可显示未知，不可编造来源。

use serde::Serialize;
use tracenest_core::CoreError;

use crate::Database;

/// 来源行为语义（用户确认可写的完整词表）。
pub mod behavior {
    pub const AI_CREATED: &str = "ai_created";
    pub const AI_MODIFIED: &str = "ai_modified";
    pub const AI_DOWNLOADED: &str = "ai_downloaded";
    pub const USER_CREATED: &str = "user_created";
    pub const USER_MODIFIED: &str = "user_modified";
    /// 合法且永久保留的状态（不可编造来源时用它）
    pub const UNKNOWN: &str = "unknown";

    /// 用户确认对话框可选择的全部行为（不含 unknown——撤销走显式 revoke）。
    pub const CONFIRMABLE: [&str; 5] = [
        AI_CREATED,
        AI_MODIFIED,
        AI_DOWNLOADED,
        USER_CREATED,
        USER_MODIFIED,
    ];

    pub fn is_valid(b: &str) -> bool {
        CONFIRMABLE.contains(&b) || b == UNKNOWN
    }
}

/// 事件语义（file_events.kind；M3-A 子集 + M3-B 预留词）。
pub mod event_kind {
    pub const SOURCE_CONFIRMED: &str = "source_confirmed";
    pub const SOURCE_CORRECTED: &str = "source_corrected";
    /// "消失 + 同 FileId 重现"的待确认标记（绝不静默改写身份）
    pub const FILE_ID_REUSE_SUSPECTED: &str = "file_id_reuse_suspected";

    /// **身份域事件注册表**：撤销身份采集时删除的 kind 全集。
    /// 规则：新增任何事件 kind 必须在此声明域归属（登记进本数组 = 身份域；
    /// 观察域 kind 差分词表见 identity_pass::DIFF_EVENT_KINDS）——未登记的
    /// 新 kind 会被域完备性测试（域注册表完备）拦截。
    pub const IDENTITY_DOMAIN_KINDS: [&str; 3] =
        [SOURCE_CONFIRMED, SOURCE_CORRECTED, FILE_ID_REUSE_SUSPECTED];
}

/// 证据语义（source_evidence.kind）。
pub mod evidence_kind {
    pub const USER_STATEMENT: &str = "user_statement";
}

/// 授权 meta 键：文件身份采集（M3-A 唯一隐私增量）默认关闭。
pub const META_IDENTITY_COLLECTION: &str = "identity_collection_enabled";

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 卷身份行（identity_volumes 查询结果）。
#[derive(Debug, Clone, Serialize)]
pub struct VolumeIdentityRow {
    pub volume_id: i64,
    pub root: String,
    pub fs_type: Option<String>,
    pub volume_guid: Option<String>,
    pub entity_count: i64,
}

/// 身份采集总览（设置页/总览页数据源；全部真实查询）。
#[derive(Debug, Clone, Serialize)]
pub struct SourceOverview {
    pub collection_enabled: bool,
    pub entities: i64,
    pub with_file_id: i64,
    pub weak_identity: i64,
    pub confirmed_entities: i64,
    pub candidates: i64,
    pub events: i64,
}

/// 撤销授权的结果统计（删除模式返回被删行数；仅停止模式全 0）。
#[derive(Debug, Clone, Serialize)]
pub struct RevokeStats {
    pub deleted_data: bool,
    pub entities: i64,
    pub events: i64,
    pub evidence: i64,
    pub candidates: i64,
    pub confirmations: i64,
}

/// 有效来源结论（来自最新一次用户确认；无确认 = None → UI 显示未知）。
#[derive(Debug, Clone, Serialize)]
pub struct EffectiveSource {
    pub behavior: String,
    pub action: String,
    pub created_at: i64,
}

/// 用户确认面板（识别中心"来源确认"弹层数据源）。
#[derive(Debug, Clone, Serialize)]
pub struct PathSourceDetail {
    pub path_id: i64,
    /// 身份是否已采集（未采集时其余字段为 None/空）
    pub entity: Option<EntityBrief>,
    pub effective: Option<EffectiveSource>,
    /// 确认历史（append-only，含被修正的旧结论）
    pub confirmations: Vec<ConfirmationRow>,
    /// 事件时间线（含证据引用；M3-A 事件 = 确认/修正/重用嫌疑）
    pub events: Vec<EventRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityBrief {
    pub id: i64,
    pub identity_kind: String,
    /// 16 进制显示的 FileId（file_id 身份时非空）
    pub file_id: Option<String>,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfirmationRow {
    pub id: i64,
    pub action: String,
    pub behavior: String,
    pub note: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub detail: Option<String>,
    pub evidence_id: Option<i64>,
}

impl Database {
    // ---------------- 授权开关（默认关闭，可暂停可撤销） ----------------

    /// 文件身份采集是否已授权（默认 false——未经授权绝不采集）。
    pub fn identity_collection_enabled(&self) -> Result<bool, CoreError> {
        Ok(self.get_meta(META_IDENTITY_COLLECTION)?.as_deref() == Some("1"))
    }

    /// 开启 / 暂停身份采集。暂停（false）不删除任何已采集数据；
    /// 数据删除只能走 [`Database::revoke_identity_collection`]（用户明确选择）。
    pub fn set_identity_collection_enabled(&self, on: bool) -> Result<(), CoreError> {
        self.set_meta(META_IDENTITY_COLLECTION, if on { "1" } else { "0" })
    }

    /// 撤销授权：停止采集（enabled=0）；`delete_data=true` 时删除全部
    /// 身份数据（五表 + attributions.entity_id 重置 + 卷 GUID 清除）。
    /// 只影响身份数据：**paths / scans / attributions 主体行一行不动**。
    pub fn revoke_identity_collection(&self, delete_data: bool) -> Result<RevokeStats, CoreError> {
        self.set_identity_collection_enabled(false)?;
        if !delete_data {
            return Ok(RevokeStats {
                deleted_data: false,
                entities: 0,
                events: 0,
                evidence: 0,
                candidates: 0,
                confirmations: 0,
            });
        }
        let count = |sql: &str| -> Result<i64, CoreError> {
            Ok(self
                .conn
                .execute(sql, [])
                .map_err(|e| CoreError::Database(format!("撤销删除失败: {e}")))?
                as i64)
        };
        // 身份域 / 观察域分治（v6，M3-B1）：
        // 1) 只删身份域事件（按 kind 域注册表，不按挂点）——**必须先删**：
        //    若先解除引用，entity-only 的身份域事件会失去唯一挂点、违反表级 CHECK；
        // 2) 再解除差分（观察域）事件的实体引用（剩余事件均有 path 挂点，安全）；
        // 3) 删除已无引用的孤儿证据（差分事件 evidence_id=NULL，其无引用的
        //    user_statement 证据随之清除；台账 scan_diff_runs 属观察域，完全不动）；
        // 4) candidates/confirmations（仅身份域）全删；attributions.entity_id/volume_guid
        //    照旧重置；最后删实体（此时已无引用）。
        let kind_list = event_kind::IDENTITY_DOMAIN_KINDS
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(",");
        let mut stats = RevokeStats {
            deleted_data: true,
            events: count(&format!(
                "DELETE FROM file_events WHERE kind IN ({kind_list})"
            ))?,
            candidates: count("DELETE FROM source_candidates")?,
            confirmations: count("DELETE FROM source_confirmations")?,
            evidence: 0,
            entities: 0,
        };
        self.conn
            .execute(
                "UPDATE file_events SET entity_id=NULL WHERE entity_id IS NOT NULL",
                [],
            )
            .map_err(|e| CoreError::Database(format!("解除观察事件实体引用失败: {e}")))?;
        stats.evidence = count(
            "DELETE FROM source_evidence WHERE id NOT IN \
             (SELECT evidence_id FROM file_events WHERE evidence_id IS NOT NULL)",
        )?;
        self.conn
            .execute("UPDATE attributions SET entity_id=NULL", [])
            .map_err(|e| CoreError::Database(format!("重置归属关联失败: {e}")))?;
        self.conn
            .execute("UPDATE volumes SET volume_guid=NULL", [])
            .map_err(|e| CoreError::Database(format!("清除卷 GUID 失败: {e}")))?;
        stats.entities = count("DELETE FROM file_entities")?;
        Ok(stats)
    }

    // ---------------- 来源总览 ----------------

    pub fn source_overview(&self) -> Result<SourceOverview, CoreError> {
        let q = |sql: &str| -> Result<i64, CoreError> {
            self.conn
                .query_row(sql, [], |r| r.get(0))
                .map_err(|e| CoreError::Database(format!("来源总览查询失败: {e}")))
        };
        Ok(SourceOverview {
            collection_enabled: self.identity_collection_enabled()?,
            entities: q("SELECT COUNT(*) FROM file_entities")?,
            with_file_id: q("SELECT COUNT(*) FROM file_entities WHERE identity_kind='file_id'")?,
            weak_identity: q("SELECT COUNT(*) FROM file_entities WHERE identity_kind='weak'")?,
            confirmed_entities: q("SELECT COUNT(DISTINCT entity_id) FROM source_confirmations")?,
            candidates: q("SELECT COUNT(*) FROM source_candidates")?,
            events: q("SELECT COUNT(*) FROM file_events")?,
        })
    }

    /// 已登记卷的身份采集视图（root/fs_type/GUID/实体计数）。
    pub fn identity_volumes(&self) -> Result<Vec<VolumeIdentityRow>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT v.id, v.root, v.fs_type, v.volume_guid,
                        (SELECT COUNT(*) FROM file_entities fe WHERE fe.volume_id=v.id)
                 FROM volumes v ORDER BY v.root",
            )
            .map_err(|e| CoreError::Database(format!("卷身份列表查询失败: {e}")))?;
        let iter = stmt
            .query_map([], |r| {
                Ok(VolumeIdentityRow {
                    volume_id: r.get(0)?,
                    root: r.get(1)?,
                    fs_type: r.get(2)?,
                    volume_guid: r.get(3)?,
                    entity_count: r.get(4)?,
                })
            })
            .map_err(|e| CoreError::Database(format!("卷身份列表读取失败: {e}")))?;
        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(format!("卷身份列表读取失败: {e}")))
    }

    // ---------------- 用户确认 / 撤销（唯一的行为写入路径） ----------------

    /// 由路径节点解析实体（未采集身份时报错，不编造）。
    fn entity_by_path(&self, path_id: i64) -> Result<i64, CoreError> {
        self.conn
            .query_row(
                "SELECT id FROM file_entities WHERE current_path_id=?1 LIMIT 1",
                [path_id],
                |r| r.get(0),
            )
            .map_err(|_| {
                CoreError::Other(format!(
                    "路径 {path_id} 尚未采集文件身份：请先在设置页授权并在总览页运行身份采集"
                ))
            })
    }

    /// 用户确认来源行为。已有确认时 action 自动记为 correct（修正历史保留）。
    /// 写入三处：证据（user_statement）→ 确认（append-only）→ 事件（时间线）。
    pub fn confirm_source(
        &self,
        path_id: i64,
        behavior: &str,
        note: Option<&str>,
    ) -> Result<EffectiveSource, CoreError> {
        if !behavior::is_valid(behavior) || behavior == behavior::UNKNOWN {
            return Err(CoreError::Other(format!(
                "非法的来源行为: {behavior}（撤销请用 revoke_source）"
            )));
        }
        let entity_id = self.entity_by_path(path_id)?;
        let existing: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM source_confirmations WHERE entity_id=?1",
                [entity_id],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("查询确认历史失败: {e}")))?;
        let action = if existing > 0 { "correct" } else { "confirm" };
        self.write_confirmation(entity_id, action, behavior, note)
    }

    /// 撤销/否定确认：记录一条 revoke（behavior=unknown），历史保留。
    pub fn revoke_source(
        &self,
        path_id: i64,
        note: Option<&str>,
    ) -> Result<EffectiveSource, CoreError> {
        let entity_id = self.entity_by_path(path_id)?;
        self.write_confirmation(entity_id, "revoke", behavior::UNKNOWN, note)
    }

    fn write_confirmation(
        &self,
        entity_id: i64,
        action: &str,
        behavior: &str,
        note: Option<&str>,
    ) -> Result<EffectiveSource, CoreError> {
        let ts = now_ms();
        // 1) 证据先行（确认可追溯的证据记录；payload 只含用户陈述本身）
        self.conn
            .execute(
                "INSERT INTO source_evidence(kind,origin,collected_at,scope,payload)
                 VALUES(?1,'user',?2,?3,?4)",
                rusqlite::params![
                    evidence_kind::USER_STATEMENT,
                    ts,
                    "仅用户主观陈述，覆盖自动候选展示",
                    serde_json::json!({ "behavior": behavior, "note": note }).to_string()
                ],
            )
            .map_err(|e| CoreError::Database(format!("写入证据失败: {e}")))?;
        let evidence_id = self.conn.last_insert_rowid();
        // 2) 确认记录（append-only）
        self.conn
            .execute(
                "INSERT INTO source_confirmations(entity_id,action,behavior,note,created_at)
                 VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![entity_id, action, behavior, note, ts],
            )
            .map_err(|e| CoreError::Database(format!("写入确认失败: {e}")))?;
        // 3) 事件（时间线可见；append-only）
        // confirm → source_confirmed；correct / revoke（撤回到 unknown 也是一次修正）→ source_corrected
        let event_kind = if action == "confirm" {
            event_kind::SOURCE_CONFIRMED
        } else {
            event_kind::SOURCE_CORRECTED
        };
        self.conn
            .execute(
                "INSERT INTO file_events(entity_id,ts,kind,evidence_id,detail)
                 VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![
                    entity_id,
                    ts,
                    event_kind,
                    evidence_id,
                    serde_json::json!({ "behavior": behavior, "action": action }).to_string()
                ],
            )
            .map_err(|e| CoreError::Database(format!("写入事件失败: {e}")))?;
        Ok(EffectiveSource {
            behavior: behavior.to_string(),
            action: action.to_string(),
            created_at: ts,
        })
    }

    // ---------------- 查询（面板 / 时间线） ----------------

    /// 路径节点的来源详情（身份 + 有效结论 + 确认历史 + 事件时间线）。
    pub fn path_source_detail(&self, path_id: i64) -> Result<PathSourceDetail, CoreError> {
        let entity = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, identity_kind, file_id, first_seen_at, last_seen_at
                     FROM file_entities WHERE current_path_id=?1 LIMIT 1",
                )
                .map_err(|e| CoreError::Database(format!("查询实体失败: {e}")))?;
            let mut rows = stmt
                .query([path_id])
                .map_err(|e| CoreError::Database(format!("查询实体失败: {e}")))?;
            match rows
                .next()
                .map_err(|e| CoreError::Database(format!("查询实体失败: {e}")))?
            {
                None => None,
                Some(r) => Some(EntityBrief {
                    id: r.get(0).map_err(|e| CoreError::Database(format!("{e}")))?,
                    identity_kind: r.get(1).map_err(|e| CoreError::Database(format!("{e}")))?,
                    file_id: r
                        .get::<_, Option<i64>>(2)
                        .map_err(|e| CoreError::Database(format!("{e}")))?
                        .map(|v| format!("{v:X}")),
                    first_seen_at: r.get(3).map_err(|e| CoreError::Database(format!("{e}")))?,
                    last_seen_at: r.get(4).map_err(|e| CoreError::Database(format!("{e}")))?,
                }),
            }
        };
        let Some(ent) = entity.as_ref() else {
            return Ok(PathSourceDetail {
                path_id,
                entity: None,
                effective: None,
                confirmations: vec![],
                events: vec![],
            });
        };
        let eid = ent.id;
        let mut confirmations: Vec<ConfirmationRow> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, action, behavior, note, created_at
                     FROM source_confirmations WHERE entity_id=?1 ORDER BY id DESC",
                )
                .map_err(|e| CoreError::Database(format!("查询确认失败: {e}")))?;
            let iter = stmt
                .query_map([eid], |r| {
                    Ok(ConfirmationRow {
                        id: r.get(0)?,
                        action: r.get(1)?,
                        behavior: r.get(2)?,
                        note: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                })
                .map_err(|e| CoreError::Database(format!("查询确认失败: {e}")))?;
            iter.collect::<Result<Vec<_>, _>>()
                .map_err(|e| CoreError::Database(format!("查询确认失败: {e}")))?
        };
        // 有效结论 = 最新一条（revoke → unknown 也是结论：诚实展示"用户已否定"）
        let effective = confirmations.first().map(|c| EffectiveSource {
            behavior: c.behavior.clone(),
            action: c.action.clone(),
            created_at: c.created_at,
        });
        let events: Vec<EventRow> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, ts, kind, detail, evidence_id
                     FROM file_events WHERE entity_id=?1 ORDER BY ts DESC, id DESC",
                )
                .map_err(|e| CoreError::Database(format!("查询事件失败: {e}")))?;
            let iter = stmt
                .query_map([eid], |r| {
                    Ok(EventRow {
                        id: r.get(0)?,
                        ts: r.get(1)?,
                        kind: r.get(2)?,
                        detail: r.get(3)?,
                        evidence_id: r.get(4)?,
                    })
                })
                .map_err(|e| CoreError::Database(format!("查询事件失败: {e}")))?;
            iter.collect::<Result<Vec<_>, _>>()
                .map_err(|e| CoreError::Database(format!("查询事件失败: {e}")))?
        };
        // 已被消费的确认历史不再重复暴露给面板（有效结论已含最新一条）
        confirmations.truncate(20);
        Ok(PathSourceDetail {
            path_id,
            entity,
            effective,
            confirmations,
            events,
        })
    }
}

// ---------------------------------------------------------------------------
// 测试：三层分模型语义 + append-only 保证 + 授权开关
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Database {
        Database::open(":memory:".as_ref()).expect("内存库应能打开")
    }

    /// 造一个"已采集身份"的场景：卷 + 扫描 + 路径 + 实体，返回 (db, path_id, entity_id)。
    fn seeded() -> (Database, i64, i64) {
        let db = mem_db();
        let vol = db
            .upsert_volume("X:\\", None, None, None, Some("NTFS"), false)
            .unwrap();
        let scan = db
            .begin_scan(&["X:\\".into()], "t", db.instance_id())
            .unwrap();
        let path_id = db
            .insert_path(scan, vol, None, "report.docx", 1, false, 100, None, 0)
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO file_entities(volume_id,identity_kind,file_id,check_size,check_mtime,current_path_id,first_seen_scan_id,last_seen_scan_id,first_seen_at,last_seen_at)
                 VALUES(?1,'file_id',4242,100,0,?2,?3,?3,0,0)",
                rusqlite::params![vol, path_id, scan],
            )
            .unwrap();
        let entity_id: i64 = db
            .conn
            .query_row("SELECT id FROM file_entities LIMIT 1", [], |r| r.get(0))
            .unwrap();
        (db, path_id, entity_id)
    }

    #[test]
    fn 授权默认关闭_开关可暂停不删数据() {
        let db = mem_db();
        assert!(!db.identity_collection_enabled().unwrap(), "默认必须关闭");
        db.set_identity_collection_enabled(true).unwrap();
        assert!(db.identity_collection_enabled().unwrap());
        // 暂停：数据保留
        db.set_identity_collection_enabled(false).unwrap();
        assert!(!db.identity_collection_enabled().unwrap());
        let n: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM meta WHERE key=?1",
                [META_IDENTITY_COLLECTION],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn 确认写入证据_确认_事件三层_且首认与修正语义正确() {
        let (db, path_id, entity_id) = seeded();
        let eff = db
            .confirm_source(path_id, behavior::AI_CREATED, Some("我看着它生成的"))
            .unwrap();
        assert_eq!(eff.action, "confirm");
        // 三层各一条
        let n_evidence: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM source_evidence", [], |r| r.get(0))
            .unwrap();
        let n_confirm: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM source_confirmations", [], |r| {
                r.get(0)
            })
            .unwrap();
        let n_events: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM file_events WHERE entity_id=?1 AND kind=?2",
                rusqlite::params![entity_id, event_kind::SOURCE_CONFIRMED],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((n_evidence, n_confirm, n_events), (1, 1, 1));
        // 事件引用证据 id（候选/面板只引用不复制）
        let ev: Option<i64> = db
            .conn
            .query_row(
                "SELECT evidence_id FROM file_events WHERE entity_id=?1",
                [entity_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(ev.is_some());
        // 修正：action 变 correct，旧确认保留（append-only）
        let eff2 = db
            .confirm_source(path_id, behavior::USER_CREATED, None)
            .unwrap();
        assert_eq!(eff2.action, "correct");
        let total: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM source_confirmations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(total, 2, "修正历史必须保留");
        // 有效结论 = 最新一条
        let detail = db.path_source_detail(path_id).unwrap();
        assert_eq!(
            detail.effective.as_ref().unwrap().behavior,
            behavior::USER_CREATED
        );
        assert_eq!(detail.effective.as_ref().unwrap().action, "correct");
        assert_eq!(detail.confirmations.len(), 2);
    }

    #[test]
    fn 撤销确认_行为记为unknown_历史保留() {
        let (db, path_id, _) = seeded();
        db.confirm_source(path_id, behavior::AI_MODIFIED, None)
            .unwrap();
        let eff = db.revoke_source(path_id, Some("认错了")).unwrap();
        assert_eq!(eff.action, "revoke");
        assert_eq!(eff.behavior, behavior::UNKNOWN);
        let detail = db.path_source_detail(path_id).unwrap();
        assert_eq!(
            detail.effective.as_ref().unwrap().behavior,
            behavior::UNKNOWN
        );
        assert_eq!(detail.confirmations.len(), 2, "revoke 也是一条历史");
    }

    #[test]
    fn 非法行为拒绝_未采集身份拒绝() {
        let (db, path_id, _) = seeded();
        // 非法词表
        assert!(db.confirm_source(path_id, "ai_maybe", None).is_err());
        // unknown 不能走确认（要走 revoke）
        assert!(db.confirm_source(path_id, behavior::UNKNOWN, None).is_err());
        // 未采集身份的路径拒绝（宁可拒绝，不编造）
        assert!(db
            .confirm_source(path_id + 999, behavior::AI_CREATED, None)
            .is_err());
    }

    #[test]
    fn 候选与确认隔离_删除候选不影响确认() {
        // 三层分模型的核心保证：候选可整体重建，绝不触碰确认
        let (db, path_id, entity_id) = seeded();
        db.confirm_source(path_id, behavior::AI_CREATED, None)
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO source_candidates(entity_id,behavior,tool_id,confidence,evidence_ids,generation,created_at)
                 VALUES(?1,'ai_modified',NULL,'possible','[]',1,0)",
                [entity_id],
            )
            .unwrap();
        // 整体重建 = 全删（M3-C 将按 generation 重算）
        db.conn
            .execute("DELETE FROM source_candidates", [])
            .unwrap();
        let detail = db.path_source_detail(path_id).unwrap();
        assert_eq!(
            detail.effective.as_ref().unwrap().behavior,
            behavior::AI_CREATED,
            "候选删除不得影响用户确认"
        );
        assert_eq!(detail.events.len(), 1);
    }

    #[test]
    fn 撤销授权_仅停止模式不删数据_删除模式清空身份域() {
        let (db, path_id, entity_id) = seeded();
        db.set_identity_collection_enabled(true).unwrap();
        db.confirm_source(path_id, behavior::AI_CREATED, None)
            .unwrap();
        db.conn
            .execute(
                "UPDATE volumes SET volume_guid='\\\\?\\Volume{x}' WHERE id=1",
                [],
            )
            .unwrap();
        // 造一条归属并关联实体（模拟 M2 数据 + v5 关联）
        db.conn
            .execute(
                "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES('codex','Codex','','cli','#111',1)",
                [],
            )
            .unwrap();
        let scan: i64 = db
            .conn
            .query_row("SELECT MAX(id) FROM scans", [], |r| r.get(0))
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior,entity_id)
                 VALUES(?1,?2,1,'verified',1.0,'{}','unknown',?3)",
                rusqlite::params![path_id, scan, entity_id],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO source_candidates(entity_id,behavior,tool_id,confidence,evidence_ids,generation,created_at)
                 VALUES(?1,'ai_created',NULL,'likely','[]',1,0)",
                rusqlite::params![entity_id],
            )
            .unwrap();

        // 仅停止：数据全保留
        let s = db.revoke_identity_collection(false).unwrap();
        assert!(!s.deleted_data && !db.identity_collection_enabled().unwrap());
        assert!(db.path_source_detail(path_id).unwrap().entity.is_some());

        // 重新开启 + 删除模式：身份域清空，paths/attributions 主体不动
        db.set_identity_collection_enabled(true).unwrap();
        let s = db.revoke_identity_collection(true).unwrap();
        assert!(s.deleted_data);
        let n = |t: &str| -> i64 {
            db.conn
                .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(
            (
                n("file_entities"),
                n("file_events"),
                n("source_evidence"),
                n("source_candidates"),
                n("source_confirmations")
            ),
            (0, 0, 0, 0, 0)
        );
        assert!(!db.identity_collection_enabled().unwrap());
        // M1/M2 数据不动：paths 保留、attributions 行保留但实体关联重置
        assert_eq!(db.stats().unwrap().paths_total, 1);
        let attr_total: i64 = n("attributions");
        assert_eq!(attr_total, 1, "归属行本身不得删除");
        let linked: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM attributions WHERE entity_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, 0, "归属实体关联应重置");
        let guid: Option<String> = db
            .conn
            .query_row("SELECT volume_guid FROM volumes WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(guid.is_none(), "卷 GUID 应清除");
    }

    #[test]
    fn 撤销_观察域差分事件与台账保留_身份域清除() {
        let (db, path_id, entity_id) = seeded();
        db.set_identity_collection_enabled(true).unwrap();
        db.confirm_source(path_id, behavior::AI_CREATED, None)
            .unwrap();
        // 造观察域：台账一行 + 差分事件（path 挂点 + entity 引用 + rel_key）
        db.conn.execute(
            "INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,created_at)
             VALUES(1,1,1,'completed',1,0)", []).unwrap();
        let run_id: i64 = db.conn.last_insert_rowid();
        db.conn.execute(
            "INSERT INTO file_events(path_id,entity_id,volume_id,rel_path,rel_key,ts,kind,diff_run_id,scan_id)
             VALUES(?1,?2,1,'report.docx','REPORT.DOCX',0,'file_discovered',?3,1)",
            rusqlite::params![path_id, entity_id, run_id],
        ).unwrap();
        let diff_event_id: i64 = db.conn.last_insert_rowid();

        let s = db.revoke_identity_collection(true).unwrap();
        assert!(s.deleted_data);
        // 观察域保留：差分事件仍在，且 entity 引用被解除（FK 安全）
        let row: (Option<i64>, Option<i64>, String) = db
            .conn
            .query_row(
                "SELECT entity_id, path_id, kind FROM file_events WHERE id=?1",
                [diff_event_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(row.0, None, "观察域事件 entity 引用必须解除");
        assert_eq!(row.1, Some(path_id), "path 挂点保留");
        assert_eq!(row.2, "file_discovered");
        // 台账（观察域）完整保留
        let runs: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM scan_diff_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(runs, 1, "差分台账属观察域，撤销不得触碰");
        // 身份域清除
        let n = |sql: &str| -> i64 { db.conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(
            n("SELECT COUNT(*) FROM file_events WHERE kind='source_confirmed'"),
            0
        );
        assert_eq!(
            (
                n("SELECT COUNT(*) FROM source_evidence"),
                n("SELECT COUNT(*) FROM source_confirmations"),
                n("SELECT COUNT(*) FROM source_candidates"),
                n("SELECT COUNT(*) FROM file_entities")
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(s.events, 1, "身份域事件删除计数");
        assert!(s.evidence >= 1, "孤儿 user_statement 证据应被清理");
    }

    #[test]
    fn 面板_未采集身份返回空壳不报错() {
        let (db, _, _) = seeded();
        let d = db.path_source_detail(987654).unwrap();
        assert!(d.entity.is_none());
        assert!(d.effective.is_none());
        assert!(d.events.is_empty());
    }
}
