//! # FMap 数据库层（tracenest-database）
//!
//! 职责：
//! - 数据库文件初始化与连接管理（WAL 模式，支持"扫描批量写 + UI 只读"并发）
//! - 版本化迁移（meta 表中记录 schema_version；v2 增加 instance_id/end_note）
//! - 扫描代生命周期：登记 → 批量事务写入（scan_writer）→ 聚合 → 终态
//! - 异常退出恢复（recover_orphaned_scans）
//!
//! 隐私约束（docs/architecture/SECURITY.md）：
//! - 只存元数据（路径/大小/时间/归属），**绝不存文件内容**
//! - 数据库视为敏感数据：位于用户私有目录，提供清除入口（后续版本）

pub mod attribution_pass;
pub mod rules_loader;
pub mod scan_writer;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use tracenest_core::CoreError;

/// 内嵌的权威 Schema（与 tests/schema.sql 同源，避免两处维护）。
/// 注意：SCHEMA_SQL 为 v1 基线；v2+ 的增量列/表由 migrate() 以 ALTER /
/// CREATE IF NOT EXISTS 应用，保证 v1 旧库与全新库收敛到同一结构。
pub const SCHEMA_SQL: &str = include_str!("../../../tests/schema.sql");

/// v5 增量 DDL（M3-A 文件资产基础模型：五张新表；与 tests/schema_v5.sql 同源）。
pub const SCHEMA_V5_SQL: &str = include_str!("../../../tests/schema_v5.sql");

/// 内置 AI 工具规则包 v1（来源 rules/builtin-rules-v1.json，含来源与依据说明）。
pub const BUILTIN_RULES_JSON: &str = include_str!("../../../rules/builtin-rules-v1.json");

/// 当前期望的 schema 版本号。每次结构变更 +1 并补充迁移分支。
pub const SCHEMA_VERSION: i64 = 5;

/// 数据库句柄。非 Clone：应用状态中以 Mutex 持有。
pub struct Database {
    conn: Connection,
    path: PathBuf,
    /// 本实例标识（进程级随机，用于异常退出恢复时的任务归属判定）
    instance_id: String,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 生成实例标识：纳秒时间戳 + 进程号（无需外部 uuid 依赖，碰撞概率可忽略）。
fn generate_instance_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}-{:x}", nanos, std::process::id())
}

impl Database {
    /// 打开（或创建）数据库并确保 schema 就绪。
    ///
    /// - parent 目录不存在时自动创建（应用数据目录由调用方决定）
    /// - `:memory:` 用于测试
    /// - 本方法不做孤儿任务恢复：那是显式的 [`Database::recover_orphaned_scans`]，
    ///   便于测试与调用方控制时机
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        if path != Path::new(":memory:") {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| CoreError::Database(format!("创建数据目录失败: {e}")))?;
            }
        }
        let conn = Connection::open(path)
            .map_err(|e| CoreError::Database(format!("打开数据库失败: {e}")))?;

        // WAL：读写并发友好（扫描写 + UI 读）。内存库不支持 journal_mode 设置，忽略错误。
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| CoreError::Database(format!("启用外键失败: {e}")))?;
        // 并发写锁等待：主连接与扫描工作线程写连接并发时，等待而非立即 SQLITE_BUSY
        let _ = conn.busy_timeout(std::time::Duration::from_millis(30_000));

        let mut db = Self {
            conn,
            path: path.to_path_buf(),
            instance_id: generate_instance_id(),
        };
        db.migrate()?;
        Ok(db)
    }

    /// 本实例标识（scan_start 登记任务时写入，用于恢复判定）。
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// 版本化迁移（崩溃安全设计）：
    ///
    /// - 每个版本阶段包在**独立事务**里，版本号写入与该阶段 DDL **同一事务**：
    ///   任一步失败 → 整体回滚 → 版本号保持旧值 → 下次启动重跑该阶段；
    /// - ALTER 前用 pragma_table_info 逐列检查，已存在的列跳过
    ///   （修复 v2→v3 半迁移事故：版本号先行、缺列导致的永久失败）；
    /// - 重复启动/重复迁移安全；历史数据不动。
    pub fn migrate(&mut self) -> Result<(), CoreError> {
        let current: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE((SELECT value FROM meta WHERE key='schema_version'),'0')",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "0".to_string())
            .parse()
            .unwrap_or(0);

        // v1：基线结构（CREATE TABLE IF NOT EXISTS 本身幂等；版本号同事务写入）
        if current < 1 {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 v1 迁移事务失败: {e}")))?;
            tx.execute_batch(SCHEMA_SQL)
                .map_err(|e| CoreError::Database(format!("应用 schema v1 失败: {e}")))?;
            Self::set_schema_version(&tx, 1)?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 v1 迁移失败: {e}")))?;
        }

        // v1 → v2：scans.instance_id（异常退出恢复的任务归属）+ scans.end_note（终态说明）
        if current < 2 {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 v2 迁移事务失败: {e}")))?;
            Self::ensure_column(&tx, "scans", "instance_id", "TEXT")?;
            Self::ensure_column(&tx, "scans", "end_note", "TEXT")?;
            Self::set_schema_version(&tx, 2)?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 v2 迁移失败: {e}")))?;
        }

        // v2 → v3：rules.enabled/weight/evidence + tools.kind
        if current < 3 {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 v3 迁移事务失败: {e}")))?;
            Self::ensure_column(&tx, "rules", "enabled", "INTEGER NOT NULL DEFAULT 1")?;
            Self::ensure_column(&tx, "rules", "weight", "REAL")?;
            Self::ensure_column(&tx, "rules", "evidence", "TEXT")?;
            Self::ensure_column(&tx, "tools", "kind", "TEXT")?;
            Self::set_schema_version(&tx, 3)?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 v3 迁移失败: {e}")))?;
        }

        // v3 → v4：**归属语义修正**——attributions 增加 source_behavior（来源行为维度）。
        // 工具关联 ≠ 文件来源行为：自动分析只给出工具关联结论；
        // 来源行为（AI_CREATED/AI_MODIFIED/AI_DOWNLOADED/USER_CREATED/UNKNOWN 等）
        // 一律 'unknown'，只有用户确认（未来功能）才能改变。既有行由 DEFAULT 填充。
        if current < 4 {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 v4 迁移事务失败: {e}")))?;
            Self::ensure_column(
                &tx,
                "attributions",
                "source_behavior",
                "TEXT NOT NULL DEFAULT 'unknown'",
            )?;
            Self::set_schema_version(&tx, 4)?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 v4 迁移失败: {e}")))?;
        }

        // v4 → v5：**M3-A 文件资产基础模型**——新增五表（file_entities /
        // source_evidence / file_events / source_candidates / source_confirmations，
        // 三层分模型见 schema_v5.sql 头注）+ volumes.volume_guid（身份键不用盘符）
        // + attributions.entity_id（M2 工具关联 → 文件实体关联，历史行为 NULL）。
        // 全部非破坏；历史数据不动；版本号与 DDL 同事务。
        if current < 5 {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 v5 迁移事务失败: {e}")))?;
            tx.execute_batch(SCHEMA_V5_SQL)
                .map_err(|e| CoreError::Database(format!("应用 schema v5 失败: {e}")))?;
            Self::ensure_column(&tx, "volumes", "volume_guid", "TEXT")?;
            Self::ensure_column(
                &tx,
                "attributions",
                "entity_id",
                "INTEGER REFERENCES file_entities(id)",
            )?;
            Self::set_schema_version(&tx, 5)?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 v5 迁移失败: {e}")))?;
        }

        // 未来版本在此追加 if current < 6 { ... }

        let actual: i64 = self
            .conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("读取 schema_version 失败: {e}")))?;
        if actual != SCHEMA_VERSION {
            return Err(CoreError::Database(format!(
                "schema 版本不匹配：期望 {SCHEMA_VERSION}，实际 {actual}"
            )));
        }
        Ok(())
    }

    /// 幂等补列：目标列不存在时执行 ALTER，已存在则跳过。
    /// 表/列名均来自本文件硬编码常量（非用户输入），无注入风险。
    fn ensure_column(
        conn: &Connection,
        table: &str,
        column: &str,
        decl: &str,
    ) -> Result<(), CoreError> {
        let exists: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name='{column}'"),
                [],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("检查列 {table}.{column} 失败: {e}")))?;
        if exists > 0 {
            return Ok(());
        }
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
            [],
        )
        .map_err(|e| CoreError::Database(format!("补列 {table}.{column} 失败: {e}")))?;
        Ok(())
    }

    /// 写入 schema 版本号（必须与该阶段的 DDL 在同一事务内调用）。
    fn set_schema_version(conn: &Connection, v: i64) -> Result<(), CoreError> {
        conn.execute(
            "INSERT INTO meta(key,value) VALUES('schema_version',?1)
             ON CONFLICT(key) DO UPDATE SET value=?1",
            rusqlite::params![v.to_string()],
        )
        .map_err(|e| CoreError::Database(format!("写入 schema_version={v} 失败: {e}")))?;
        Ok(())
    }

    /// 异常退出恢复：把**不属于本实例**的 running 扫描标记为 failed 并写明原因。
    ///
    /// - 已入库的 paths 记录全部保留（只改 scans 终态，不删任何数据）
    /// - 返回被恢复的扫描代 id 列表
    /// - 边界：M1/M2 假设单实例扫描；双实例并发的进程存活检测列后续计划
    pub fn recover_orphaned_scans(&self) -> Result<Vec<i64>, CoreError> {
        let recovered: Vec<i64> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id FROM scans
                     WHERE status='running' AND (instance_id IS NULL OR instance_id <> ?1)",
                )
                .map_err(|e| CoreError::Database(format!("孤儿扫描查询失败: {e}")))?;
            let iter = stmt
                .query_map([self.instance_id.as_str()], |r| r.get(0))
                .map_err(|e| CoreError::Database(format!("孤儿扫描映射失败: {e}")))?;
            iter.collect::<Result<Vec<_>, _>>()
                .map_err(|e| CoreError::Database(format!("孤儿扫描读取失败: {e}")))?
        };
        for id in &recovered {
            self.finish_scan(
                *id,
                "failed",
                0,
                0,
                0,
                Some("应用在扫描期间退出（启动时恢复，已完成部分已保留）"),
            )?;
        }
        Ok(recovered)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 登记一个卷（已存在则更新容量信息），返回其 id。
    pub fn upsert_volume(
        &self,
        root: &str,
        label: Option<&str>,
        total_bytes: Option<u64>,
        free_bytes: Option<u64>,
        fs_type: Option<&str>,
        is_removable: bool,
    ) -> Result<i64, CoreError> {
        self.conn
            .execute(
                "INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(root) DO UPDATE SET
                   label=excluded.label,
                   total_bytes=excluded.total_bytes,
                   free_bytes=excluded.free_bytes,
                   fs_type=excluded.fs_type,
                   is_removable=excluded.is_removable",
                rusqlite::params![
                    root,
                    label,
                    total_bytes.map(|v| v as i64),
                    free_bytes.map(|v| v as i64),
                    fs_type,
                    is_removable as i64,
                    now_ms()
                ],
            )
            .map_err(|e| CoreError::Database(format!("登记卷失败: {e}")))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 创建一条扫描任务记录（登记本实例标识），返回扫描代 id。
    pub fn begin_scan(
        &self,
        roots: &[String],
        app_version: &str,
        instance_id: &str,
    ) -> Result<i64, CoreError> {
        self.conn
            .execute(
                "INSERT INTO scans(started_at,status,roots,app_version,instance_id)
                 VALUES(?1,'running',?2,?3,?4)",
                rusqlite::params![
                    now_ms(),
                    serde_json::to_string(roots).unwrap_or_default(),
                    app_version,
                    instance_id
                ],
            )
            .map_err(|e| CoreError::Database(format!("创建扫描记录失败: {e}")))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 插入单个路径节点（扫描线程按 DFS 先序调用：父先于子），返回新节点 id。
    #[allow(clippy::too_many_arguments)]
    pub fn insert_path(
        &self,
        scan_id: i64,
        volume_id: i64,
        parent_id: Option<i64>,
        name: &str,
        depth: i64,
        is_dir: bool,
        size: u64,
        mtime: Option<i64>,
        attrs: u32,
    ) -> Result<i64, CoreError> {
        Self::insert_path_on(
            &self.conn, scan_id, volume_id, parent_id, name, depth, is_dir, size, mtime, attrs,
        )
    }

    /// 事务内插入（供 scan_writer 批量事务使用；conn 可为 Transaction 的 Deref）。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert_path_on(
        conn: &Connection,
        scan_id: i64,
        volume_id: i64,
        parent_id: Option<i64>,
        name: &str,
        depth: i64,
        is_dir: bool,
        size: u64,
        mtime: Option<i64>,
        attrs: u32,
    ) -> Result<i64, CoreError> {
        conn.execute(
            "INSERT INTO paths(volume_id,parent_id,name,depth,is_dir,size,mtime,attrs,scan_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                volume_id,
                parent_id,
                name,
                depth,
                is_dir as i64,
                size as i64,
                mtime,
                attrs as i64,
                scan_id
            ],
        )
        .map_err(|e| CoreError::Database(format!("插入路径失败: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// 批量事务写入：单事务内插入一批路径，成功提交 / 失败整体回滚。
    /// 返回成功插入的行数。这是"不出现半提交"的保证点。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert_paths_tx(
        conn: &Connection,
        scan_id: i64,
        volume_id: i64,
        batch: &[tracenest_core::scanner::WalkEntry],
        rel_ids: &mut std::collections::HashMap<String, i64>,
    ) -> Result<usize, CoreError> {
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| CoreError::Database(format!("开启事务失败: {e}")))?;
        for e in batch {
            let (parent_rel, name) = match e.rel_path.rsplit_once('/') {
                Some((p, n)) => (p.to_string(), n.to_string()),
                None => (String::new(), e.name.clone()),
            };
            let parent_id = rel_ids.get(&parent_rel).copied();
            let id = Self::insert_path_on(
                &tx,
                scan_id,
                volume_id,
                parent_id,
                &name,
                e.depth as i64 + 1,
                e.is_dir,
                e.size,
                e.mtime,
                e.attrs,
            )?;
            if e.is_dir {
                rel_ids.insert(e.rel_path.clone(), id);
            }
        }
        tx.commit()
            .map_err(|e| CoreError::Database(format!("提交批次失败: {e}")))?;
        Ok(batch.len())
    }

    /// 取最新一条 running 扫描代的 DB id（仅诊断用途；扫描流程已改为显式传递 id）。
    pub fn query_latest_running_scan(&self) -> Result<i64, CoreError> {
        self.conn
            .query_row(
                "SELECT id FROM scans WHERE status='running' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("查询运行中扫描失败: {e}")))
    }

    /// 确保卷已登记并返回其 id（不存在则插入最小记录）。
    pub fn ensure_volume(&self, root: &str) -> Result<i64, CoreError> {
        use rusqlite::OptionalExtension;
        let existing: Option<i64> = self
            .conn
            .query_row("SELECT id FROM volumes WHERE root=?1", [root], |r| r.get(0))
            .optional()
            .map_err(|e| CoreError::Database(format!("查询卷失败: {e}")))?;
        if let Some(id) = existing {
            return Ok(id);
        }
        self.upsert_volume(root, None, None, None, None, false)
    }

    /// 扫描完成后：目录 size = 子树聚合（自底向上单趟，按深度降序）。
    pub fn aggregate_dir_sizes(&self, scan_id: i64) -> Result<(), CoreError> {
        let rows: Vec<(i64, Option<i64>, i64)> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, parent_id, size FROM paths
                     WHERE scan_id = ?1 AND deleted_at IS NULL
                     ORDER BY depth DESC",
                )
                .map_err(|e| CoreError::Database(format!("聚合查询失败: {e}")))?;
            let iter = stmt
                .query_map([scan_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(|e| CoreError::Database(format!("聚合映射失败: {e}")))?;
            iter.collect::<Result<Vec<_>, _>>()
                .map_err(|e| CoreError::Database(format!("聚合读取失败: {e}")))?
        };
        // child id -> 累计增量；自底向上累加到父节点
        let mut delta: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
        for (id, parent, size) in rows {
            let total = size + delta.remove(&id).unwrap_or(0);
            if total > 0 {
                self.conn
                    .execute(
                        "UPDATE paths SET size=?1 WHERE id=?2",
                        rusqlite::params![total, id],
                    )
                    .map_err(|e| CoreError::Database(format!("聚合回写失败: {e}")))?;
            }
            if let Some(p) = parent {
                *delta.entry(p).or_insert(0) += total;
            }
        }
        Ok(())
    }

    /// 结束扫描任务（status 遵循七态模型；end_note 记录终态补充说明）。
    pub fn finish_scan(
        &self,
        scan_id: i64,
        status: &str,
        files_scanned: u64,
        dirs_scanned: u64,
        errors_count: u64,
        end_note: Option<&str>,
    ) -> Result<(), CoreError> {
        self.conn
            .execute(
                "UPDATE scans SET finished_at=?1, status=?2, files_scanned=?3, dirs_scanned=?4, errors_count=?5, end_note=?6
                 WHERE id=?7",
                rusqlite::params![
                    now_ms(),
                    status,
                    files_scanned as i64,
                    dirs_scanned as i64,
                    errors_count as i64,
                    end_note,
                    scan_id
                ],
            )
            .map_err(|e| CoreError::Database(format!("更新扫描记录失败: {e}")))?;
        Ok(())
    }

    /// M0 验收用的基础统计（首页展示，全部来自真实数据）。
    pub fn stats(&self) -> Result<DbStats, CoreError> {
        let q = |sql: &str| -> Result<i64, CoreError> {
            self.conn
                .query_row(sql, [], |r| r.get(0))
                .map_err(|e| CoreError::Database(format!("统计查询失败: {e}")))
        };
        Ok(DbStats {
            schema_version: q(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
            )?,
            volumes: q("SELECT COUNT(*) FROM volumes")?,
            scans_total: q("SELECT COUNT(*) FROM scans")?,
            scans_running: q("SELECT COUNT(*) FROM scans WHERE status='running'")?,
            paths_total: q("SELECT COUNT(*) FROM paths")?,
        })
    }

    /// 读取一个 meta 键值（应用级设置；key 为本 crate 硬编码常量，无注入风险）。
    pub fn get_meta(&self, key: &str) -> Result<Option<String>, CoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM meta WHERE key=?1")
            .map_err(|e| CoreError::Database(format!("读取 meta 失败: {e}")))?;
        let mut rows = stmt
            .query([key])
            .map_err(|e| CoreError::Database(format!("读取 meta 失败: {e}")))?;
        match rows
            .next()
            .map_err(|e| CoreError::Database(format!("读取 meta 失败: {e}")))?
        {
            Some(r) => {
                Ok(Some(r.get(0).map_err(|e| {
                    CoreError::Database(format!("读取 meta 失败: {e}"))
                })?))
            }
            None => Ok(None),
        }
    }

    /// 写入一个 meta 键值（UPSERT）。
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.conn
            .execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=?2",
                rusqlite::params![key, value],
            )
            .map_err(|e| CoreError::Database(format!("写入 meta 失败: {e}")))?;
        Ok(())
    }

    /// 记录卷 GUID（身份键基础；采集开启后由身份解析取得，撤销授权时可清除）。
    pub fn set_volume_guid(&self, volume_id: i64, guid: Option<&str>) -> Result<(), CoreError> {
        self.conn
            .execute(
                "UPDATE volumes SET volume_guid=?1 WHERE id=?2",
                rusqlite::params![guid, volume_id],
            )
            .map_err(|e| CoreError::Database(format!("写入卷 GUID 失败: {e}")))?;
        Ok(())
    }
}

/// 首页"系统状态"数据（全部真实查询，无任何演示值）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DbStats {
    pub schema_version: i64,
    pub volumes: i64,
    pub scans_total: i64,
    pub scans_running: i64,
    pub paths_total: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Database {
        Database::open(Path::new(":memory:")).expect("内存库应能打开")
    }

    #[test]
    fn 空库初始化_版本为5_各表为空() {
        let db = mem_db();
        let s = db.stats().expect("stats 应成功");
        assert_eq!(s.schema_version, 5);
        assert_eq!(s.volumes, 0);
        assert_eq!(s.scans_total, 0);
        assert_eq!(s.paths_total, 0);
    }

    #[test]
    fn 卷登记_幂等更新() {
        let db = mem_db();
        let id1 = db
            .upsert_volume(
                "C:\\",
                Some("系统"),
                Some(100),
                Some(50),
                Some("NTFS"),
                false,
            )
            .unwrap();
        let id2 = db
            .upsert_volume(
                "C:\\",
                Some("系统盘"),
                Some(200),
                Some(80),
                Some("NTFS"),
                false,
            )
            .unwrap();
        assert_eq!(id1, id2, "同一 root 应更新而非新增");
        let s = db.stats().unwrap();
        assert_eq!(s.volumes, 1);
    }

    #[test]
    fn 扫描记录_完整生命周期_含end_note() {
        let db = mem_db();
        let scan_id = db
            .begin_scan(&["D:\\".into()], "0.1.0-test", db.instance_id())
            .unwrap();
        let s0 = db.stats().unwrap();
        assert_eq!(s0.scans_running, 1, "刚创建应为 running");
        db.finish_scan(scan_id, "partial", 10, 2, 1, Some("测试终态说明"))
            .unwrap();
        let s = db.stats().unwrap();
        assert_eq!(s.scans_total, 1);
        assert_eq!(s.scans_running, 0, "结束后不应再有 running");
    }

    #[test]
    fn 外键约束生效() {
        let db = mem_db();
        // 没有 volume 1 时插入 path 应被外键拒绝
        let r = db.conn.execute(
            "INSERT INTO paths(volume_id,name,depth,is_dir,scan_id) VALUES(1,'x',0,0,1)",
            [],
        );
        assert!(r.is_err(), "外键开启时插入孤立 path 必须失败");
    }

    #[test]
    fn 迁移幂等_重复打开不报错() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        {
            let mut db = Database::open(&p).unwrap();
            db.migrate().unwrap();
        }
        {
            let db = Database::open(&p).unwrap();
            assert_eq!(
                db.stats().unwrap().schema_version,
                5,
                "二次打开不应重复迁移"
            );
        }
    }

    #[test]
    fn v1旧库迁移到最新版_历史数据保留() {
        // 模拟一个真实的 v1 旧库：v1 结构 + 一条 v1 扫描记录 + 若干路径
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("legacy.db");
        {
            let raw = Connection::open(&p).unwrap();
            raw.execute_batch(SCHEMA_SQL).unwrap(); // v1 基线
            raw.execute(
                "INSERT INTO meta(key,value) VALUES('schema_version','1')",
                [],
            )
            .unwrap();
            raw.execute(
                "INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen)
                 VALUES('E:\\','文档',1,1,'NTFS',0,0)",
                [],
            )
            .unwrap();
            raw.execute(
                "INSERT INTO scans(started_at,status,roots,app_version) VALUES(0,'running','[\"E:\\\\\"]','0.1.0')",
                [],
            )
            .unwrap();
            raw.execute(
                "INSERT INTO paths(volume_id,parent_id,name,depth,is_dir,size,scan_id) VALUES(1,NULL,'E:',0,1,0,1)",
                [],
            )
            .unwrap();
        }
        // 旧库被新版打开 → 自动迁移到最新版
        let db = Database::open(&p).unwrap();
        assert_eq!(
            db.stats().unwrap().schema_version,
            5,
            "旧库应迁移到最新版本"
        );
        assert_eq!(db.stats().unwrap().paths_total, 1, "迁移不得丢失 paths");
        // 旧 running 扫描可被恢复函数回收（instance_id 为 NULL）
        let recovered = db.recover_orphaned_scans().unwrap();
        assert_eq!(recovered.len(), 1, "孤儿 running 应被回收");
        let row: (String, Option<String>) = db
            .conn
            .query_row("SELECT status, end_note FROM scans WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(row.0, "failed");
        assert!(row.1.unwrap().contains("退出"), "end_note 应说明恢复原因");
    }

    #[test]
    fn 恢复函数_不误伤本实例任务() {
        let db = mem_db();
        let own = db
            .begin_scan(&["C:\\".into()], "t", db.instance_id())
            .unwrap();
        let foreign = db
            .begin_scan(&["D:\\".into()], "t", "dead-instance-xyz")
            .unwrap();
        let recovered = db.recover_orphaned_scans().unwrap();
        assert_eq!(recovered, vec![foreign], "只回收非本实例的 running");
        let own_status: String = db
            .conn
            .query_row("SELECT status FROM scans WHERE id=?1", [own], |r| r.get(0))
            .unwrap();
        assert_eq!(own_status, "running", "本实例任务不得被误标");
    }

    #[test]
    fn v4迁移_既有归属行填充_source_behavior_unknown() {
        let db = mem_db();
        db.upsert_volume("X:\\", None, None, None, Some("NTFS"), false)
            .unwrap();
        let volume_id = db.ensure_volume("X:\\").unwrap();
        let scan_db_id = db
            .begin_scan(&["X:\\".into()], "t", db.instance_id())
            .unwrap();
        let root_id = db
            .insert_path(scan_db_id, volume_id, None, "X:", 0, true, 0, None, 0)
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES('t','T','','','',1)",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis)
                 VALUES(?1,?2,1,'verified',1.0,'{}')",
                rusqlite::params![root_id, scan_db_id],
            )
            .unwrap();
        // v4 迁移在 open 时已执行：既有行应被 DEFAULT 填充为 unknown
        let sb: String = db
            .conn
            .query_row(
                "SELECT source_behavior FROM attributions WHERE path_id=?1",
                [root_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sb, "unknown", "自动归属的来源行为必须诚实标注未知");
    }

    #[test]
    fn 迁移崩溃恢复_半迁移状态可安全续跑() {
        // 模拟 v2 阶段崩溃：instance_id 已加、end_note 未加、版本号停在 1（事务回滚态）
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("crash.db");
        {
            let raw = Connection::open(&p).unwrap();
            raw.execute_batch(SCHEMA_SQL).unwrap();
            Database::set_schema_version(&raw, 1).unwrap();
            // 崩溃残留：仅第一个 ALTER 已执行
            raw.execute("ALTER TABLE scans ADD COLUMN instance_id TEXT", [])
                .unwrap();
            raw.execute(
                "INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen)
                 VALUES('E:\','文档',1,1,'NTFS',0,0)",
                [],
            )
            .unwrap();
            raw.execute(
                "INSERT INTO scans(started_at,status,roots,app_version,instance_id) VALUES(0,'running','[\"E:\\\\\"]','0.1.0','dead')",
                [],
            )
            .unwrap();
            raw.execute(
                "INSERT INTO paths(volume_id,parent_id,name,depth,is_dir,size,scan_id) VALUES(1,NULL,'E:',0,1,0,1)",
                [],
            )
            .unwrap();
        }
        // 崩溃后重新打开：应安全续跑（补 end_note → v3 → v4 → v5），数据保留
        let db = Database::open(&p).unwrap();
        assert_eq!(db.stats().unwrap().schema_version, 5);
        assert_eq!(db.stats().unwrap().paths_total, 1, "崩溃恢复不得丢失数据");
        // 遗留 running（instance_id='dead' ≠ 当前实例）→ 启动恢复由调用方触发；此处仅验证列齐全
        let cols: Vec<String> = db
            .conn
            .prepare("SELECT name FROM pragma_table_info('scans')")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(cols.contains(&"end_note".to_string()), "缺失列应被补齐");
        assert!(
            cols.contains(&"instance_id".to_string()),
            "既有列不得重复添加而报错"
        );
        let rec = db.recover_orphaned_scans().unwrap();
        assert_eq!(rec, vec![1], "崩溃遗留 running 应被识别回收");
    }

    #[test]
    fn 迁移幂等_列已存在时跳过不报错() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        {
            let db = Database::open(&p).unwrap(); // 已迁移到 v5
        }
        let db = Database::open(&p).unwrap();
        assert_eq!(db.stats().unwrap().schema_version, 5);
    }

    /// 构造一个真实的 v4 库（v1 基线 + v2/v3/v4 增量列 + 样例数据），
    /// 用于验证 v4 → v5 迁移（这是要跑在用户 50 万行真实库上的路径）。
    fn make_v4_legacy_db(p: &Path) {
        let raw = Connection::open(p).unwrap();
        raw.execute_batch(SCHEMA_SQL).unwrap();
        // v2 增量
        raw.execute("ALTER TABLE scans ADD COLUMN instance_id TEXT", [])
            .unwrap();
        raw.execute("ALTER TABLE scans ADD COLUMN end_note TEXT", [])
            .unwrap();
        // v3 增量
        raw.execute(
            "ALTER TABLE rules ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
            [],
        )
        .unwrap();
        raw.execute("ALTER TABLE rules ADD COLUMN weight REAL", [])
            .unwrap();
        raw.execute("ALTER TABLE rules ADD COLUMN evidence TEXT", [])
            .unwrap();
        raw.execute("ALTER TABLE tools ADD COLUMN kind TEXT", [])
            .unwrap();
        // v4 增量
        raw.execute(
            "ALTER TABLE attributions ADD COLUMN source_behavior TEXT NOT NULL DEFAULT 'unknown'",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO meta(key,value) VALUES('schema_version','4')",
            [],
        )
        .unwrap();
        // 样例数据：卷 / 扫描 / 路径 / 工具 / 归属
        raw.execute(
            "INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen)
             VALUES('E:\\','文档',1,1,'NTFS',0,0)",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO scans(started_at,status,roots,app_version) VALUES(0,'done','[\"E:\\\\\"]','0.1.0')",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO paths(volume_id,parent_id,name,depth,is_dir,size,scan_id)
             VALUES(1,NULL,'E:',0,1,0,1),
                   (1,1,'.codex',1,1,0,1)",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES('codex','Codex','','cli','#111',1)",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior)
             VALUES(2,1,1,'verified',1.0,'{}','unknown')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn v4旧库迁移到v5_历史数据保留且五表可用() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("v4.db");
        make_v4_legacy_db(&p);
        let counts_before: (i64, i64, i64) = {
            let raw = Connection::open(&p).unwrap();
            (
                raw.query_row("SELECT COUNT(*) FROM paths", [], |r| r.get(0))
                    .unwrap(),
                raw.query_row("SELECT COUNT(*) FROM attributions", [], |r| r.get(0))
                    .unwrap(),
                raw.query_row("SELECT COUNT(*) FROM tools", [], |r| r.get(0))
                    .unwrap(),
            )
        };

        let db = Database::open(&p).unwrap();
        assert_eq!(db.stats().unwrap().schema_version, 5);
        // 历史数据一行不少
        assert_eq!(db.stats().unwrap().paths_total, counts_before.0);
        let attr_n: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM attributions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(attr_n, counts_before.1);
        // v5 五表存在且可用（外键生效：孤立实体插入必须被拒）
        assert!(
            db.conn
                .execute(
                    "INSERT INTO file_events(entity_id,ts,kind) VALUES(999,0,'x')",
                    []
                )
                .is_err(),
            "file_events.entity_id 外键必须生效"
        );
        // entity 关联列可写（ attributions.entity_id → file_entities.id ）
        db.conn
            .execute(
                "INSERT INTO file_entities(volume_id,identity_kind,file_id,first_seen_at,last_seen_at)
                 VALUES(1,'file_id',42,0,0)",
                [],
            )
            .unwrap();
        db.conn
            .execute("UPDATE attributions SET entity_id=1 WHERE path_id=2", [])
            .unwrap();
        let linked: Option<i64> = db
            .conn
            .query_row(
                "SELECT entity_id FROM attributions WHERE path_id=2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, Some(1));
    }

    #[test]
    fn v5半迁移状态_部分表已建可安全续跑() {
        // 模拟 v5 阶段中断后的残留：v4 库 + file_entities 已建（但版本号仍是 4）
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("half-v5.db");
        make_v4_legacy_db(&p);
        {
            let raw = Connection::open(&p).unwrap();
            // 手动应用 v5 DDL 的一部分（如真实崩溃后残留——虽然同事务回滚下
            // 理论不会发生，此处验证"部分表已存在"仍可收敛）
            raw.execute_batch(
                "CREATE TABLE IF NOT EXISTS file_entities (
                    id INTEGER PRIMARY KEY,
                    volume_id INTEGER NOT NULL REFERENCES volumes(id),
                    identity_kind TEXT NOT NULL,
                    file_id INTEGER,
                    check_size INTEGER,
                    check_mtime INTEGER,
                    weak_path TEXT,
                    current_path_id INTEGER REFERENCES paths(id),
                    first_seen_scan_id INTEGER REFERENCES scans(id),
                    last_seen_scan_id INTEGER REFERENCES scans(id),
                    first_seen_at INTEGER NOT NULL,
                    last_seen_at INTEGER NOT NULL,
                    deleted_at INTEGER
                 );",
            )
            .unwrap();
        }
        let db = Database::open(&p).unwrap();
        assert_eq!(db.stats().unwrap().schema_version, 5);
        // 重复建表不报错、其余四表补齐、数据保留
        let tables: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN
                 ('file_entities','file_events','source_evidence','source_candidates','source_confirmations')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 5, "五张 v5 表必须齐全");
        assert_eq!(db.stats().unwrap().paths_total, 2);
    }

    #[test]
    fn meta读写与卷guid() {
        let db = mem_db();
        assert_eq!(db.get_meta("identity_collection_enabled").unwrap(), None);
        db.set_meta("identity_collection_enabled", "1").unwrap();
        assert_eq!(
            db.get_meta("identity_collection_enabled").unwrap(),
            Some("1".into())
        );
        db.set_meta("identity_collection_enabled", "0").unwrap();
        assert_eq!(
            db.get_meta("identity_collection_enabled").unwrap(),
            Some("0".into())
        );
        // 卷 GUID
        let vol = db
            .upsert_volume("E:\\", None, None, None, Some("NTFS"), false)
            .unwrap();
        db.set_volume_guid(vol, Some(r"\\?\Volume{1234abcd-11}"))
            .unwrap();
        let guid: Option<String> = db
            .conn
            .query_row("SELECT volume_guid FROM volumes WHERE id=?1", [vol], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(guid.as_deref(), Some(r"\\?\Volume{1234abcd-11}"));
        db.set_volume_guid(vol, None).unwrap();
        let guid: Option<String> = db
            .conn
            .query_row("SELECT volume_guid FROM volumes WHERE id=?1", [vol], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(guid.is_none(), "撤销授权时可清除 GUID");
    }
}
