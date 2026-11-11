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
pub mod backup;
pub mod browse;
pub mod browser;
pub mod correlation_pass;
pub mod diff_pass;
pub mod identity_pass;
pub mod projects;
pub mod reports;
pub mod rules_loader;
pub mod scan_writer;
pub mod source;
pub mod watch;

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

/// v6 增量 DDL（M3-B1 基础事件追踪：差分台账 + file_events 重建；与 tests/schema_v6.sql 同源）。
pub const SCHEMA_V6_SQL: &str = include_str!("../../../tests/schema_v6.sql");

/// v7 增量 DDL（M3-C 工具行为关联：correlation_candidates；与 tests/schema_v7.sql 同源）。
pub const SCHEMA_V7_SQL: &str = include_str!("../../../tests/schema_v7.sql");

/// v8 增量 DDL（M3-D 浏览器下载记录：授权源 + 导入记录；与 tests/schema_v8.sql 同源）。
pub const SCHEMA_V8_SQL: &str = include_str!("../../../tests/schema_v8.sql");

/// v9 增量 DDL（M5 性能：全局查询索引；与 tests/schema_v9.sql 同源）。
pub const SCHEMA_V9_SQL: &str = include_str!("../../../tests/schema_v9.sql");

/// 内置 AI 工具规则包 v1（来源 rules/builtin-rules-v1.json，含来源与依据说明）。
pub const BUILTIN_RULES_JSON: &str = include_str!("../../../rules/builtin-rules-v1.json");

/// 当前期望的 schema 版本号。每次结构变更 +1 并补充迁移分支。
pub const SCHEMA_VERSION: i64 = 9;

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

        // v5 → v6：**M3-B1 基础文件事件追踪**——scan_diff_runs 差分执行台账 +
        // file_events 重建（挂点可空化、逻辑路径列、表级 CHECK、diff_run_id 关联、
        // entity_id 去 CASCADE——观察域事件在身份撤销时保留）+ scans.scan_scope /
        // coverage_gaps。全部在阶段事务内原子完成；版本号与 DDL 同事务。
        if current < 6 {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 v6 迁移事务失败: {e}")))?;
            tx.execute_batch(SCHEMA_V6_SQL)
                .map_err(|e| CoreError::Database(format!("应用 schema v6 失败: {e}")))?;
            Self::ensure_column(&tx, "scans", "scan_scope", "TEXT")?;
            Self::ensure_column(&tx, "scans", "coverage_gaps", "TEXT")?;
            Self::set_schema_version(&tx, 6)?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 v6 迁移失败: {e}")))?;
        }

        // v6 → v7：**M3-C 工具行为关联**——correlation_candidates 候选表
        //（时间+路径双上下文门槛的观察关联；审查状态机 pending/confirmed/denied/ignored；
        // 重建时已审查行保留）。候选属第二层派生数据，绝不携带来源行为；
        // 全部非破坏；历史数据不动；版本号与 DDL 同事务。
        if current < 7 {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 v7 迁移事务失败: {e}")))?;
            tx.execute_batch(SCHEMA_V7_SQL)
                .map_err(|e| CoreError::Database(format!("应用 schema v7 失败: {e}")))?;
            Self::set_schema_version(&tx, 7)?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 v7 迁移失败: {e}")))?;
        }

        // v7 → v8：**M3-D 浏览器下载记录**——browser_download_sources 授权表
        //（逐浏览器 × 逐档案；无行 = 完全关闭）+ browser_downloads 导入记录
        //（最小字段集，不含浏览上下文；候选匹配状态）。全部非破坏；
        // 版本号与 DDL 同事务。
        if current < 8 {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 v8 迁移事务失败: {e}")))?;
            tx.execute_batch(SCHEMA_V8_SQL)
                .map_err(|e| CoreError::Database(format!("应用 schema v8 失败: {e}")))?;
            Self::set_schema_version(&tx, 8)?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 v8 迁移失败: {e}")))?;
        }

        // v8 → v9：**M5 性能完善**——补全局查询索引（file_events 时间线 /
        // 关联候选 / 下载记录浏览），全部 CREATE INDEX IF NOT EXISTS 非破坏；
        // 版本号与 DDL 同事务。
        if current < 9 {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 v9 迁移事务失败: {e}")))?;
            tx.execute_batch(SCHEMA_V9_SQL)
                .map_err(|e| CoreError::Database(format!("应用 schema v9 失败: {e}")))?;
            Self::set_schema_version(&tx, 9)?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 v9 迁移失败: {e}")))?;
        }

        // 未来版本在此追加 if current < 10 { ... }

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
        // v6（M3-B1）：登记入口规范化（大写化 + 反斜杠统一 + 尾分隔符）——
        // 大小写/分隔符变体归并到同一卷行（T50）。
        // 存量兼容：先按规范化键查等价存量行——
        //   唯一匹配 → 复用原 volume_id（按 id 更新，**不改存量 root 字符串**，
        //              历史 paths/attributions 的卷关联一行不动）；
        //   多条匹配 → 拒绝并要求人工处理（绝不静默新增第三条重复登记）。
        let norm = tracenest_core::pathnorm::normalize_root(root);
        let existing: Vec<(i64, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, root FROM volumes")
                .map_err(|e| CoreError::Database(format!("登记卷失败: {e}")))?;
            let it = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                .map_err(|e| CoreError::Database(format!("登记卷失败: {e}")))?;
            it.collect::<Result<Vec<_>, _>>()
                .map_err(|e| CoreError::Database(format!("登记卷失败: {e}")))?
        };
        let matches: Vec<i64> = existing
            .iter()
            .filter(|(_, r)| tracenest_core::pathnorm::normalize_root(r) == norm)
            .map(|(id, _)| *id)
            .collect();
        match matches.len() {
            0 => {
                self.conn
                    .execute(
                        "INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen)
                         VALUES(?1,?2,?3,?4,?5,?6,?7)",
                        rusqlite::params![
                            norm,
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
            1 => {
                self.conn
                    .execute(
                        "UPDATE volumes SET label=?1, total_bytes=?2, free_bytes=?3,
                           fs_type=?4, is_removable=?5 WHERE id=?6",
                        rusqlite::params![
                            label,
                            total_bytes.map(|v| v as i64),
                            free_bytes.map(|v| v as i64),
                            fs_type,
                            is_removable as i64,
                            matches[0]
                        ],
                    )
                    .map_err(|e| CoreError::Database(format!("登记卷失败: {e}")))?;
                Ok(matches[0])
            }
            _ => Err(CoreError::Other(format!(
                "存在 {} 条规范化等价的卷登记（root 互为大小写/分隔符变体），                 拒绝自动合并：请人工处理后重试（ids={:?}）",
                matches.len(),
                matches
            ))),
        }
    }

    /// 全部登记根（监控对账的卷定位用）。
    pub fn all_volume_roots(&self) -> Vec<(i64, String)> {
        let mut stmt = match self
            .conn
            .prepare("SELECT id, root FROM volumes ORDER BY id")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let it = match stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))) {
            Ok(i) => i,
            Err(_) => return Vec::new(),
        };
        it.filter_map(|r| r.ok()).collect()
    }

    /// 写入扫描代的范围快照（roots + 排除规则 + 符号链接策略）。
    /// 差分以"两代 scope 规范化一致"为 MISSING 判定前置条件。
    pub fn set_scan_scope(
        &self,
        scan_id: i64,
        scope: &tracenest_core::scan_scope::ScanScope,
    ) -> Result<(), CoreError> {
        self.conn
            .execute(
                "UPDATE scans SET scan_scope=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(scope).unwrap_or_default(), scan_id],
            )
            .map_err(|e| CoreError::Database(format!("写入扫描范围失败: {e}")))?;
        Ok(())
    }

    /// 写入扫描代的覆盖缺口清单（权限拒绝/条目失败/排除目录；相对登记根）。
    pub fn set_scan_coverage(
        &self,
        scan_id: i64,
        gaps: &tracenest_core::scan_scope::CoverageGaps,
    ) -> Result<(), CoreError> {
        self.conn
            .execute(
                "UPDATE scans SET coverage_gaps=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(gaps).unwrap_or_default(), scan_id],
            )
            .map_err(|e| CoreError::Database(format!("写入覆盖缺口失败: {e}")))?;
        Ok(())
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

    /// FTS 全文索引重建（该卷 = 指定扫描代）。
    /// 语义：该卷全部旧 FTS 行删除 → 最新代全部未删除路径写入。
    /// 此前 paths_fts 从未被写入（schema 预留"Rust 层批量维护"未实现），
    /// M4 文件搜索接线时补齐；失败不破坏 paths 数据（调用方决定策略）。
    ///
    /// 分块提交（真实库实测教训：单事务写入 ~100 万行 FTS 在 WAL 下长时间
    /// 停滞且无 I/O 进展）——每块独立小事务，WAL 正常落盘、进度可观测、
    /// 中断后重扫可重建（幂等：先清该卷全部旧行）。
    pub fn fts_rebuild_for_volume(&self, volume_id: i64, scan_id: i64) -> Result<(), CoreError> {
        {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 FTS 清理事务失败: {e}")))?;
            tx.execute(
                "DELETE FROM paths_fts WHERE rowid IN (SELECT id FROM paths WHERE volume_id=?1)",
                [volume_id],
            )
            .map_err(|e| CoreError::Database(format!("清理旧 FTS 失败: {e}")))?;
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 FTS 清理失败: {e}")))?;
        }
        let rows: Vec<(i64, String)> = {
            let mut reader = self
                .conn
                .prepare("SELECT id, name FROM paths WHERE scan_id=?1 AND deleted_at IS NULL")
                .map_err(|e| CoreError::Database(format!("读取路径名失败: {e}")))?;
            let it = reader
                .query_map([scan_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| CoreError::Database(format!("读取路径名失败: {e}")))?;
            let mut v = Vec::new();
            for r in it {
                v.push(r.map_err(|e| CoreError::Database(format!("读取路径名失败: {e}")))?);
            }
            v
        };
        const CHUNK: usize = 10_000;
        for chunk in rows.chunks(CHUNK) {
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| CoreError::Database(format!("开启 FTS 块事务失败: {e}")))?;
            for (id, name) in chunk {
                tx.execute(
                    "INSERT INTO paths_fts(rowid, name) VALUES(?1, ?2)",
                    rusqlite::params![id, name],
                )
                .map_err(|e| CoreError::Database(format!("写入 FTS 失败: {e}")))?;
            }
            tx.commit()
                .map_err(|e| CoreError::Database(format!("提交 FTS 块失败: {e}")))?;
        }
        Ok(())
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
    fn 空库初始化_版本为9_各表为空() {
        let db = mem_db();
        let s = db.stats().expect("stats 应成功");
        assert_eq!(s.schema_version, 9);
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
                9,
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
            9,
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
        assert_eq!(db.stats().unwrap().schema_version, 9);
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
            let _db = Database::open(&p).unwrap(); // 已迁移到 v5
        }
        let db = Database::open(&p).unwrap();
        assert_eq!(db.stats().unwrap().schema_version, 9);
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
            "INSERT INTO scans(started_at,status,roots,app_version) VALUES(0,'done','[]','0.1.0')",
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
        assert_eq!(db.stats().unwrap().schema_version, 9);
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
        assert_eq!(db.stats().unwrap().schema_version, 9);
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
    fn t50_卷登记规范化_变体归并_存量复用_冲突拒绝() {
        let db = mem_db();
        let id1 = db
            .upsert_volume("e:/project", None, None, None, None, false)
            .unwrap();
        // 大小写/分隔符/尾分隔符变体 → 复用同一行（不新增）
        let id2 = db
            .upsert_volume("E:\\PROJECT", None, None, None, None, false)
            .unwrap();
        assert_eq!(id1, id2, "大小写与分隔符变体必须归并到同一卷");
        assert_eq!(db.stats().unwrap().volumes, 1);
        // 存量 root 字符串不被修改（历史数据不动原则）
        let stored: String = db
            .conn
            .query_row("SELECT root FROM volumes WHERE id=?1", [id1], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, "E:\\PROJECT\\", "新登记以规范化形式存储");
        // 盘根语义保持
        let id3 = db
            .upsert_volume("X:", None, None, None, None, false)
            .unwrap();
        let stored3: String = db
            .conn
            .query_row("SELECT root FROM volumes WHERE id=?1", [id3], |r| r.get(0))
            .unwrap();
        assert!(
            stored3.ends_with('\\') || stored3.ends_with('/'),
            "根必须有尾分隔符"
        );
        // 不同路径仍是不同卷
        let id4 = db
            .upsert_volume("E:/other", None, None, None, None, false)
            .unwrap();
        assert_ne!(id1, id4);
    }

    #[test]
    fn t33_事件kind域注册完备() {
        use crate::diff_pass::kinds;
        use crate::source::event_kind;
        // 身份域 ∩ 观察域 = ∅（两域互斥）
        for k in event_kind::IDENTITY_DOMAIN_KINDS {
            assert!(
                !kinds::ALL.contains(&k),
                "kind {k} 同时出现在身份域与观察域——域注册表冲突"
            );
        }
        // 全部已知 kind 都被某域收录（新增 kind 未登记 → 此测试失败提醒）
        let all_known = [
            event_kind::SOURCE_CONFIRMED,
            event_kind::SOURCE_CORRECTED,
            event_kind::FILE_ID_REUSE_SUSPECTED,
            kinds::DISCOVERED,
            kinds::MODIFIED,
            kinds::MISSING,
            kinds::REAPPEARED,
        ];
        for k in all_known {
            let in_identity = event_kind::IDENTITY_DOMAIN_KINDS.contains(&k);
            let in_observation = kinds::ALL.contains(&k);
            assert!(in_identity || in_observation, "kind {k} 未登记任何域");
        }
    }

    // ==================== M5 完善轮：历史分页 / 超限处置 ====================

    /// 种子：1 卷 3 条差分事件（挂点齐全：path_id + volume_id + scan_id）。
    fn seed_history(db: &Database) -> (i64, i64) {
        let vol = db
            .upsert_volume("E:\\", None, None, None, None, false)
            .unwrap();
        let scan = db
            .begin_scan(&["E:\\".into()], "t", db.instance_id())
            .unwrap();
        for (i, (rel, kind)) in [
            ("a.txt", "file_discovered"),
            ("b.txt", "file_modified"),
            ("c.txt", "file_discovered"),
        ]
        .iter()
        .enumerate()
        {
            let pid = db
                .insert_path(scan, vol, None, rel, 1, false, i as u64, Some(0), 0)
                .unwrap();
            db.conn
                .execute(
                    "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,scan_id,detail)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,'{}')",
                    rusqlite::params![pid, vol, rel, rel.to_uppercase(), 1000 + i as i64, kind, scan],
                )
                .unwrap();
        }
        (vol, scan)
    }

    #[test]
    fn t60_历史事件分页_卷过滤_路径搜索() {
        let db = mem_db();
        let (vol, _) = seed_history(&db);
        // 全量：3 条，total=3
        let (rows, total) = db.history_events_paged(None, None, 100, 0).unwrap();
        assert_eq!((rows.len(), total), (3, 3));
        assert_eq!(rows[0].rel_path.as_deref(), Some("c.txt"), "时间倒序");
        // 分页：limit 2 offset 1 → total=3，2 行
        let (rows, total) = db.history_events_paged(None, None, 2, 1).unwrap();
        assert_eq!((rows.len(), total), (2, 3));
        assert_eq!(rows[0].rel_path.as_deref(), Some("b.txt"));
        // 搜索：'b' → 1 条
        let (rows, total) = db.history_events_paged(None, Some("b"), 100, 0).unwrap();
        assert_eq!((rows.len(), total), (1, 1));
        assert_eq!(rows[0].rel_path.as_deref(), Some("b.txt"));
        // 卷过滤：vol1 → 3 条；不存在卷 → 0 条
        let (_, total) = db.history_events_paged(Some(vol), None, 100, 0).unwrap();
        assert_eq!(total, 3);
        let (_, total) = db.history_events_paged(Some(9999), None, 100, 0).unwrap();
        assert_eq!(total, 0);
        // LIKE 通配符在搜索词中被剥离（防误匹配）
        let (_, total) = db.history_events_paged(None, Some("%"), 100, 0).unwrap();
        assert_eq!(total, 0, "'%' 应被剥离而非当通配符");
    }

    #[test]
    fn t61_差分上限可配置与非法值回退() {
        let db = mem_db();
        assert_eq!(
            diff_pass::configured_max_diff_events(&db),
            diff_pass::DEFAULT_MAX_DIFF_EVENTS,
            "默认 = 常量"
        );
        // 合法提升
        let eff = diff_pass::diff_raise_limit(&db, 80_000).unwrap();
        assert_eq!(eff, 80_000);
        assert_eq!(diff_pass::configured_max_diff_events(&db), 80_000);
        // 非法值拒绝
        assert!(diff_pass::diff_raise_limit(&db, 10).is_err());
        assert!(diff_pass::diff_raise_limit(&db, 99_999_999).is_err());
        // 越界存量值回退默认（配置损坏不得瘫痪差分）
        db.set_meta(diff_pass::META_DIFF_MAX_EVENTS, "5").unwrap();
        assert_eq!(
            diff_pass::configured_max_diff_events(&db),
            diff_pass::DEFAULT_MAX_DIFF_EVENTS
        );
    }

    #[test]
    fn t62_差分对放弃_仅pending可转_方向合法() {
        let db = mem_db();
        let vol = db
            .upsert_volume("E:\\", None, None, None, None, false)
            .unwrap();
        let s1 = db
            .begin_scan(&["E:\\".into()], "t", db.instance_id())
            .unwrap();
        let s2 = db
            .begin_scan(&["E:\\".into()], "t", db.instance_id())
            .unwrap();
        let s3 = db
            .begin_scan(&["E:\\".into()], "t", db.instance_id())
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,created_at,skip_reason)
                 VALUES(?1,?2,?3,'pending_over_limit',0,0,'差分事件 99999/50000 超过上限'),
                       (?1,?3,?4,'completed',5,0,NULL)",
                rusqlite::params![vol, s1, s2, s3],
            )
            .unwrap();
        let pending_id: i64 = db
            .conn
            .query_row(
                "SELECT id FROM scan_diff_runs WHERE status='pending_over_limit'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // 放弃 pending 行 → skipped_over_limit + 原因
        assert!(diff_pass::diff_abandon_pair(&db, pending_id, "误报，确认放弃").unwrap());
        let (st, reason): (String, String) = db
            .conn
            .query_row(
                "SELECT status, skip_reason FROM scan_diff_runs WHERE id=?1",
                [pending_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(st, "skipped_over_limit");
        assert!(reason.contains("人工确认放弃"));
        // 已处理行不可再放弃（append-only 方向保护）
        let completed_id: i64 = db
            .conn
            .query_row(
                "SELECT id FROM scan_diff_runs WHERE status='completed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!diff_pass::diff_abandon_pair(&db, completed_id, "x").unwrap());
        // 不存在的 id → false（不伪装成功）
        assert!(!diff_pass::diff_abandon_pair(&db, 99999, "x").unwrap());
    }

    #[test]
    fn t54_扫描范围与覆盖缺口写入读取() {
        let db = mem_db();
        let scan = db
            .begin_scan(&["X:\\".into()], "t", db.instance_id())
            .unwrap();
        let scope = tracenest_core::scan_scope::ScanScope {
            roots: vec!["X:\\".into()],
            exclude_dir_names: vec!["$RECYCLE.BIN".into()],
            follow_symlinks: false,
        };
        db.set_scan_scope(scan, &scope).unwrap();
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
        let read: (Option<String>, Option<String>) = db
            .conn
            .query_row(
                "SELECT scan_scope, coverage_gaps FROM scans WHERE id=?1",
                [scan],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(read.0.as_deref().unwrap_or("").contains("X:"));
        assert!(read.1.as_deref().unwrap_or("").contains("locked"));
    }

    // ==================== v6 迁移（M3-B1）专项测试 ====================

    /// 构造一个真实的 v5 库：v5 全部结构 + 身份域 file_events 事件（entity 挂点）。
    fn make_v5_legacy_db(p: &Path) {
        let raw = Connection::open(p).unwrap();
        raw.execute_batch(SCHEMA_SQL).unwrap();
        raw.execute_batch(SCHEMA_V5_SQL).unwrap();
        // v2~v4 增量列
        raw.execute("ALTER TABLE scans ADD COLUMN instance_id TEXT", [])
            .unwrap();
        raw.execute("ALTER TABLE scans ADD COLUMN end_note TEXT", [])
            .unwrap();
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
        raw.execute(
            "ALTER TABLE attributions ADD COLUMN source_behavior TEXT NOT NULL DEFAULT 'unknown'",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO meta(key,value) VALUES('schema_version','5')",
            [],
        )
        .unwrap();
        // 样例：卷/扫描/路径/实体/工具/归属/身份域事件
        raw.execute(
            "INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen)
             VALUES('E:\','文档',1,1,'NTFS',0,0)",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO scans(started_at,status,roots,app_version) VALUES(0,'done','[]','0.1.0')",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO paths(volume_id,parent_id,name,depth,is_dir,size,scan_id)
             VALUES(1,NULL,'E:',0,1,0,1),(1,1,'.codex',1,1,0,1)",
            [],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES('codex','Codex','','cli','#111',1)",
            [],
        ).unwrap();
        raw.execute(
            "INSERT INTO file_entities(volume_id,identity_kind,file_id,check_size,check_mtime,current_path_id,first_seen_scan_id,last_seen_scan_id,first_seen_at,last_seen_at)
             VALUES(1,'file_id',4242,0,0,2,1,1,0,0)",
            [],
        ).unwrap();
        raw.execute(
            "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior)
             VALUES(2,1,1,'verified',1.0,'{}','unknown')",
            [],
        ).unwrap();
        // 身份域事件两条（确认 + 重用嫌疑），entity 挂点
        raw.execute(
            "INSERT INTO file_events(entity_id,ts,kind,evidence_id,scan_id,detail)
             VALUES(1,100,'source_confirmed',NULL,1,'{}'),
                   (1,200,'file_id_reuse_suspected',NULL,NULL,'{}')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn v5旧库迁移到v6_事件保留且台账结构可用() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("v5.db");
        make_v5_legacy_db(&p);
        let before: i64 = {
            let raw = Connection::open(&p).unwrap();
            raw.query_row("SELECT COUNT(*) FROM file_events", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(before, 2, "迁移前应有 2 条身份域事件");

        let db = Database::open(&p).unwrap();
        assert_eq!(db.stats().unwrap().schema_version, 9);
        // 事件保留 + volume 经 实体→当前路径 推导（current_path_id=2 → volume 1）
        let rows: Vec<(i64, Option<i64>, Option<i64>, String)> = {
            let mut stmt = db
                .conn
                .prepare("SELECT id, entity_id, volume_id, kind FROM file_events ORDER BY id")
                .unwrap();
            let it = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap();
            it.collect::<Result<Vec<_>, _>>().unwrap()
        };
        assert_eq!(rows.len(), 2, "事件一行不少");
        for (_, entity_id, volume_id, _kind) in &rows {
            assert!(entity_id.is_some(), "旧身份域事件保留 entity 挂点");
            assert_eq!(*volume_id, Some(1), "volume 应经 实体→当前路径 推导");
        }
        // 台账表存在且可用
        db.conn.execute(
            "INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,created_at)
             VALUES(1,1,1,'completed',0,0)", []).unwrap();
        // UNIQUE(volume_id, to_scan_id)
        assert!(db.conn.execute(
            "INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,created_at)
             VALUES(1,1,1,'completed',0,0)", []).is_err(), "台账唯一约束必须生效");
    }

    #[test]
    fn v6约束_挂点CHECK与diff_run外键与事件唯一索引() {
        let db = mem_db();
        let vol = db
            .upsert_volume("X:\\", None, None, None, Some("NTFS"), false)
            .unwrap();
        let scan = db
            .begin_scan(&["X:\\".into()], "t", db.instance_id())
            .unwrap();
        let path_id = db
            .insert_path(scan, vol, None, "a.txt", 1, false, 1, None, 0)
            .unwrap();
        db.conn.execute(
            "INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,created_at)
             VALUES(?1,?2,?2,'completed',0,0)", rusqlite::params![vol, scan]).unwrap();
        let run_id: i64 = db.conn.last_insert_rowid();
        // T41：双挂点皆空 → CHECK 拒绝
        assert!(
            db.conn
                .execute(
                    "INSERT INTO file_events(ts,kind) VALUES(0,'file_discovered')",
                    []
                )
                .is_err(),
            "双 NULL 挂点必须被表级 CHECK 拒绝"
        );
        // 差分事件：path 挂点 + volume/rel_key + diff_run 关联
        db.conn.execute(
            "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,diff_run_id,scan_id)
             VALUES(?1,?2,'a.txt','A.TXT',0,'file_discovered',?3,?4)",
            rusqlite::params![path_id, vol, run_id, scan]).unwrap();
        // T54：不存在的 diff_run_id → FK 拒绝
        assert!(db.conn.execute(
            "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,diff_run_id,scan_id)
             VALUES(?1,?2,'b.txt','B.TXT',0,'file_discovered',99999,?3)",
            rusqlite::params![path_id, vol, scan]).is_err(), "diff_run_id 外键必须生效");
        // 事件唯一索引：同 (kind,path_id,scan_id) 二次插入拒绝
        assert!(db.conn.execute(
            "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,diff_run_id,scan_id)
             VALUES(?1,?2,'a.txt','A.TXT',1,'file_discovered',?3,?4)",
            rusqlite::params![path_id, vol, run_id, scan]).is_err(), "事件唯一索引必须生效");
        // path 挂点必须带 volume（CHECK 2）
        assert!(db.conn.execute(
            "INSERT INTO file_events(path_id,ts,kind,scan_id) VALUES(?1,0,'file_discovered',?2)",
            rusqlite::params![path_id, scan]).is_err(), "path 挂点缺 volume 必须被 CHECK 拒绝");
    }

    #[test]
    fn v6脏残留_临时表预存在时迁移安全收敛() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dirty.db");
        make_v5_legacy_db(&p);
        {
            let raw = Connection::open(&p).unwrap();
            // 模拟异常残留：迁移自己的临时表名已存在（正常原子事务下不应发生）
            raw.execute_batch("CREATE TABLE file_events_v6(id INTEGER PRIMARY KEY)")
                .unwrap();
        }
        let db = Database::open(&p).unwrap();
        assert_eq!(
            db.stats().unwrap().schema_version,
            9,
            "脏残留应被 DROP IF EXISTS 清理后正常收敛"
        );
        let ev: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM file_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ev, 2, "真实数据不受脏残留清理影响");
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

// ==================== M3-B1 查询 API（历史追踪页数据源） ====================

/// 差分台账行（观察域）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffRunRow {
    pub id: i64,
    pub volume_id: i64,
    pub from_scan_id: i64,
    pub to_scan_id: i64,
    pub status: String,
    pub events_count: i64,
    pub ambiguous_count: i64,
    pub skip_reason: Option<String>,
    pub created_at: i64,
}

/// 历史事件行（观察域差分事件）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEventRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub volume_id: Option<i64>,
    pub rel_path: Option<String>,
    pub detail: Option<String>,
    pub entity_id: Option<i64>,
}

impl Database {
    /// 差分台账查询（volume_id=None = 全部卷）。
    pub fn diff_runs(&self, volume_id: Option<i64>) -> Result<Vec<DiffRunRow>, CoreError> {
        let sql = "SELECT id, volume_id, from_scan_id, to_scan_id, status, events_count, ambiguous_count, skip_reason, created_at FROM scan_diff_runs";
        let (query, vol) = match volume_id {
            Some(v) => (
                format!("{sql} WHERE volume_id=?1 ORDER BY to_scan_id DESC, id DESC"),
                Some(v),
            ),
            None => (format!("{sql} ORDER BY to_scan_id DESC, id DESC"), None),
        };
        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| CoreError::Database(format!("台账查询失败: {e}")))?;
        let map = |r: &rusqlite::Row| -> rusqlite::Result<DiffRunRow> {
            Ok(DiffRunRow {
                id: r.get(0)?,
                volume_id: r.get(1)?,
                from_scan_id: r.get(2)?,
                to_scan_id: r.get(3)?,
                status: r.get(4)?,
                events_count: r.get(5)?,
                ambiguous_count: r.get(6)?,
                skip_reason: r.get(7)?,
                created_at: r.get(8)?,
            })
        };
        let it = match vol {
            Some(v) => stmt
                .query_map([v], map)
                .map_err(|e| CoreError::Database(format!("{e}")))?,
            None => stmt
                .query_map([], map)
                .map_err(|e| CoreError::Database(format!("{e}")))?,
        };
        it.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(format!("台账读取失败: {e}")))
    }

    /// 历史差分事件查询 v2（只读观察域；时间倒序 + 分页 + 卷过滤 + 路径子串搜索）。
    /// 返回 (当前页行, 满足条件的总数)——总数供前端分页控件使用。
    /// search 匹配 rel_path 子串（LIKE，ASCII 大小写不敏感；空串/None = 不过滤）。
    #[allow(clippy::type_complexity)]
    pub fn history_events_paged(
        &self,
        volume_id: Option<i64>,
        search: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<HistoryEventRow>, i64), CoreError> {
        let lim = limit.clamp(1, 500);
        let off = offset.max(0);
        let search_term = search
            .map(str::trim)
            .filter(|s| !s.is_empty())
            // LIKE 通配符按字面匹配（ESCAPE '#'——路径中不出现 '#'，避开
            // Rust/SQL 双层反斜杠转义；搜索 "50%" 不会变成"50任意"）
            .map(|s| {
                format!(
                    "%{}%",
                    s.replace('#', "##").replace('%', "#%").replace('_', "#_")
                )
            });
        // 条件拼装（占位符按出现顺序绑定：volume → search）
        let mut conds: Vec<String> = vec!["rel_path IS NOT NULL".into()];
        if volume_id.is_some() {
            conds.push("volume_id=?".into());
        }
        if search_term.is_some() {
            conds.push("rel_path LIKE ? ESCAPE '#'".into());
        }
        let where_sql = conds.join(" AND ");
        let total: i64 = {
            let sql = format!("SELECT COUNT(*) FROM file_events WHERE {where_sql}");
            let mut stmt = self
                .conn
                .prepare(&sql)
                .map_err(|e| CoreError::Database(format!("事件计数失败: {e}")))?;
            match (&volume_id, &search_term) {
                (Some(v), Some(s)) => stmt.query_row(rusqlite::params![v, s], |r| r.get(0)),
                (Some(v), None) => stmt.query_row([v], |r| r.get(0)),
                (None, Some(s)) => stmt.query_row([s], |r| r.get(0)),
                (None, None) => stmt.query_row([], |r| r.get(0)),
            }
            .map_err(|e| CoreError::Database(format!("事件计数失败: {e}")))?
        };
        let sql = format!(
            "SELECT id, ts, kind, volume_id, rel_path, detail, entity_id
             FROM file_events WHERE {where_sql}
             ORDER BY ts DESC, id DESC LIMIT {} OFFSET {}",
            lim, off
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| CoreError::Database(format!("事件查询失败: {e}")))?;
        let it = match (&volume_id, &search_term) {
            (Some(v), Some(s)) => stmt.query_map(rusqlite::params![v, s], map_event_row),
            (Some(v), None) => stmt.query_map([v], map_event_row),
            (None, Some(s)) => stmt.query_map([s], map_event_row),
            (None, None) => stmt.query_map([], map_event_row),
        }
        .map_err(|e| CoreError::Database(format!("事件查询失败: {e}")))?;
        let mut out = Vec::new();
        for r in it {
            out.push(r.map_err(|e| CoreError::Database(format!("事件读取失败: {e}")))?);
        }
        Ok((out, total))
    }
}

/// 事件行映射（history_events_paged 用）。
fn map_event_row(r: &rusqlite::Row) -> rusqlite::Result<HistoryEventRow> {
    Ok(HistoryEventRow {
        id: r.get(0)?,
        ts: r.get(1)?,
        kind: r.get(2)?,
        volume_id: r.get(3)?,
        rel_path: r.get(4)?,
        detail: r.get(5)?,
        entity_id: r.get(6)?,
    })
}

// ==================== M5 总览页：最近发现的资产 ====================

/// 最近发现的资产行（总览页"最近发现的资产"区块数据源；真实差分事件）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecentAssetRow {
    pub event_id: i64,
    pub ts: i64,
    /// 卷根相对路径
    pub rel_path: String,
    /// 快照绝对路径（卷根 + 名称链；展示用）
    pub abs_path: String,
    pub volume_root: String,
    /// 文件大小（字节；快照值，路径行缺失时 None）
    pub size: Option<i64>,
}

impl Database {
    /// 最近"首次观察到"的资产（file_discovered，时间倒序）。
    /// 附绝对路径（批量构建，消除 N+1）与快照大小；无事件 → 空列表（诚实空态）。
    pub fn recent_discovered_assets(&self, limit: i64) -> Result<Vec<RecentAssetRow>, CoreError> {
        let lim = limit.clamp(1, 50);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT e.id, e.ts, e.rel_path, e.volume_id, v.root, p.size, e.path_id
                 FROM file_events e
                 JOIN volumes v ON v.id = e.volume_id
                 LEFT JOIN paths p ON p.id = e.path_id
                 WHERE e.kind = 'file_discovered'
                 ORDER BY e.ts DESC, e.id DESC LIMIT ?1",
            )
            .map_err(|e| CoreError::Database(format!("最近资产查询失败: {e}")))?;
        let it = stmt
            .query_map([lim], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                ))
            })
            .map_err(|e| CoreError::Database(format!("最近资产查询失败: {e}")))?;
        type RecentRawRow = (i64, i64, String, String, Option<i64>, Option<i64>);
        let mut raw: Vec<RecentRawRow> = Vec::new();
        for row in it {
            raw.push(row.map_err(|e| CoreError::Database(format!("最近资产读取失败: {e}")))?);
        }
        // 批量绝对路径（一次祖先层加载）
        let ids: Vec<i64> = raw.iter().filter_map(|r| r.5).collect();
        let abs_map = self.abs_paths_bulk(&ids).unwrap_or_default();
        let mut out: Vec<RecentAssetRow> = Vec::new();
        for (event_id, ts, rel_path, volume_root, size, path_id) in raw {
            let abs = path_id
                .and_then(|pid| abs_map.get(&pid).cloned())
                .unwrap_or_else(|| format!("{volume_root}{rel_path}"));
            out.push(RecentAssetRow {
                event_id,
                ts,
                rel_path: rel_path.clone(),
                abs_path: abs,
                volume_root,
                size,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod recent_asset_tests {
    use super::*;

    #[test]
    fn t63_最近发现的资产_时间倒序_绝对路径_含快照大小() {
        let db = Database::open(":memory:".as_ref()).unwrap();
        let vol = db
            .upsert_volume("E:\\", None, None, None, None, false)
            .unwrap();
        let scan = db
            .begin_scan(&["E:\\".into()], "t", db.instance_id())
            .unwrap();
        let root = db
            .insert_path(scan, vol, None, "E:", 0, true, 0, Some(0), 0)
            .unwrap();
        let novel = db
            .insert_path(scan, vol, Some(root), "NOVEL", 1, true, 0, Some(0), 0)
            .unwrap();
        let c3 = db
            .insert_path(scan, vol, Some(novel), "c3.md", 2, false, 300, Some(0), 0)
            .unwrap();
        let c4 = db
            .insert_path(scan, vol, Some(novel), "c4.md", 2, false, 400, Some(0), 0)
            .unwrap();
        // 三条 discovered 事件（时间 100/200/300）
        for (pid, rel, ts) in [(c3, "NOVEL/c3.md", 100), (c4, "NOVEL/c4.md", 200)] {
            db.conn.execute(
                "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,scan_id,detail)
                 VALUES(?1,?2,?3,?4,?5,'file_discovered',?6,'{}')",
                rusqlite::params![pid, vol, rel, rel.to_uppercase(), ts, scan],
            ).unwrap();
        }
        // 一条老事件时间最早（检验倒序）
        db.conn
            .execute(
                "INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,scan_id,detail)
             VALUES(?1,?2,'NOVEL/c2.md','NOVEL/C2.MD',50,'file_discovered',?3,'{}')",
                rusqlite::params![novel, vol, scan],
            )
            .unwrap();
        let rows = db.recent_discovered_assets(10).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].abs_path, r"E:\NOVEL\c4.md", "最新在前 + 绝对路径");
        assert_eq!(rows[0].size, Some(400));
        // limit 钳制
        assert_eq!(db.recent_discovered_assets(2).unwrap().len(), 2);
        assert_eq!(
            db.recent_discovered_assets(0).unwrap().len(),
            1,
            "0 → 钳为 1"
        );
    }

    #[test]
    fn t64_无事件_空列表_诚实空态() {
        let db = Database::open(":memory:".as_ref()).unwrap();
        assert!(db.recent_discovered_assets(10).unwrap().is_empty());
    }
}
