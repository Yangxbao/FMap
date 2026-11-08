//! # 扫描差分引擎（M3-B1 核心）
//!
//! 职责：对某卷的**有效扫描代**（status='done'）做相邻差分，产出四类基础
//! 文件事件（file_discovered / file_modified / file_missing / file_reappeared）
//! 并维护 `scan_diff_runs` 执行台账（游标的唯一事实来源）。
//!
//! 设计基准：docs/M3_B_DESIGN.md v6。语义红线：
//! - DISCOVERED =「首次观察到」，≠ 刚刚创建（prev_gap 标记见事件 detail）；
//! - MISSING ≠ 删除（S_curr 覆盖不完整时**不产生**，宁漏勿误）；
//! - 不产 FILE_DELETED；不推断文件来源；工具关联不写入事件；
//! - 事件 append-only；台账/事件原子提交；pending_over_limit 独立状态事务并
//!   **阻塞**该卷后续差分（线性历史保证缺失集推导正确）；
//! - 逻辑路径键 = (volume_id, rel_key)；规范化键碰撞（ß/SS 类）→ 歧义隔离，
//!   绝不静默合并不同文件。
//!
//! 依赖现状（诚实标注）：scan_scope / coverage_gaps 由 B1-2 的扫描流程写入，
//! 本模块只读取解析——v6 之前的历史代无 scope，差分按"scope 缺失 → 跳过该对"
//! 保守处理（预期行为：真实库上线后需两次新扫描才产生首个差分对）。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tracenest_core::pathnorm::{self, normalize};
use tracenest_core::scan_scope::{CoverageGaps, ScanScope};
use tracenest_core::CoreError;

use crate::Database;

/// 差分事件词表（观察域；撤销身份采集时保留——域分治见 source.rs）。
pub mod kinds {
    pub const DISCOVERED: &str = "file_discovered";
    pub const MODIFIED: &str = "file_modified";
    pub const MISSING: &str = "file_missing";
    pub const REAPPEARED: &str = "file_reappeared";
    /// 差分白名单（缺失集推导只认这四类——其他 kind 永不干扰）。
    pub const ALL: [&str; 4] = [DISCOVERED, MODIFIED, MISSING, REAPPEARED];
}

/// 单对差分事件数量上限（默认值；生产可在 meta 中按卷全局覆盖）。
pub const DEFAULT_MAX_DIFF_EVENTS: usize = 50_000;

/// meta 键：人工确认的差分事件上限（pending_over_limit 处置"提高上限"写入）。
pub const META_DIFF_MAX_EVENTS: &str = "diff_max_events";

/// 读取当前生效的差分事件上限（meta 覆盖 → 默认值；越界值回退默认——
/// 非法配置不得让差分瘫痪）。
pub fn configured_max_diff_events(db: &Database) -> usize {
    let v = db
        .get_meta(META_DIFF_MAX_EVENTS)
        .ok()
        .flatten()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_DIFF_EVENTS);
    if (1_000..=5_000_000).contains(&v) {
        v
    } else {
        DEFAULT_MAX_DIFF_EVENTS
    }
}

/// 处置一：人工提高差分事件上限（pending_over_limit 对将由 run_diff_now
/// 自动重试——PairDecision::Run）。返回生效值（越界请求被钳制后如实返回）。
pub fn diff_raise_limit(db: &Database, new_max: usize) -> Result<usize, CoreError> {
    if !(1_000..=5_000_000).contains(&new_max) {
        return Err(CoreError::Other(format!(
            "上限需在 1,000 ~ 5,000,000 之间: {new_max}"
        )));
    }
    db.set_meta(META_DIFF_MAX_EVENTS, &new_max.to_string())?;
    Ok(new_max)
}

/// 处置二：人工确认放弃一个被阻塞的差分对（pending_over_limit →
/// skipped_over_limit，append-only 方向合法：pending 本就是"待人工处置"态）。
/// 返回是否发生转移（不存在/非 pending → false，不静默伪装成功）。
pub fn diff_abandon_pair(db: &Database, run_id: i64, note: &str) -> Result<bool, CoreError> {
    let reason = format!("人工确认放弃：{note}");
    let n = db
        .conn
        .execute(
            "UPDATE scan_diff_runs SET status='skipped_over_limit', skip_reason=?1
             WHERE id=?2 AND status='pending_over_limit'",
            rusqlite::params![reason, run_id],
        )
        .map_err(|e| CoreError::Database(format!("放弃差分对失败: {e}")))?;
    Ok(n > 0)
}

/// 台账状态（与 scan_diff_runs.status 对应）。
pub mod run_status {
    pub const COMPLETED: &str = "completed";
    pub const SKIPPED_SCOPE: &str = "skipped_scope_mismatch";
    pub const SKIPPED_OVER_LIMIT: &str = "skipped_over_limit";
    pub const PENDING_OVER_LIMIT: &str = "pending_over_limit";
    /// 已处理状态（允许后续对继续）。
    pub fn is_processed(s: &str) -> bool {
        s == COMPLETED || s == SKIPPED_SCOPE || s == SKIPPED_OVER_LIMIT
    }
}

/// 差分进度/摘要（真实计数，无演示值）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct DiffSummary {
    /// pending|running|completed|blocked|cancelled|failed
    pub status: String,
    pub pairs_completed: u64,
    pub pairs_skipped: u64,
    pub events_total: u64,
    pub discovered: u64,
    pub modified: u64,
    pub missing: u64,
    pub reappeared: u64,
    /// 因名称规范化歧义暂缓追踪的路径数（不合并、不产事件）
    pub ambiguous: u64,
    /// 被覆盖缺口/截断抑制的 MISSING 候选数（宁漏勿误的可见证据）
    pub missing_suppressed: u64,
    /// 阻塞中的差分对（pending_over_limit）
    pub blocked_pair: Option<(i64, i64)>,
    pub error: Option<String>,
}

/// 单个逻辑路径的节点信息（某扫描代中）。
#[derive(Debug, Clone)]
struct NodeInfo {
    path_id: i64,
    rel_path: String,
    is_dir: bool,
    size: i64,
    mtime: Option<i64>,
}

/// 代集合：规范化键 → 节点列表（长度 > 1 即规范化歧义）。
type GenMap = HashMap<String, Vec<NodeInfo>>;

/// IMMEDIATE 写事务助手（无 unsafe；失败 Drop 自动 ROLLBACK）。
struct ImmediateTx<'a> {
    conn: &'a rusqlite::Connection,
    finished: bool,
}

impl<'a> ImmediateTx<'a> {
    fn begin(conn: &'a rusqlite::Connection) -> Result<Self, CoreError> {
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| CoreError::Database(format!("开启 IMMEDIATE 事务失败: {e}")))?;
        Ok(Self {
            conn,
            finished: false,
        })
    }
    fn commit(mut self) -> Result<(), CoreError> {
        self.conn
            .execute_batch("COMMIT")
            .map_err(|e| CoreError::Database(format!("提交失败: {e}")))?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for ImmediateTx<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 对一个卷执行差分：循环处理所有待处理相邻对，直到无对/取消/失败/阻塞。
///
/// 单独事务语义：每对一个原子事务（事件+台账）；pending_over_limit 为独立
/// 状态事务（仅写台账单行）。取消在**对间**检查（事务内不打断，保原子）。
pub fn run_scan_diff(
    db: &Database,
    volume_id: i64,
    cancel: Option<&AtomicBool>,
    max_events: usize,
) -> Result<DiffSummary, CoreError> {
    let mut summary = DiffSummary {
        status: "running".into(),
        ..Default::default()
    };

    loop {
        if let Some(flag) = cancel {
            if flag.load(Ordering::Relaxed) {
                summary.status = "cancelled".into();
                return Ok(summary);
            }
        }
        // 选对（确定性：done 代升序 + 台账连续前缀 + pending 阻塞）
        let pair = match next_pending_pair(db, volume_id)? {
            PairDecision::None => {
                summary.status = "completed".into();
                return Ok(summary);
            }
            PairDecision::Blocked(from, to) => {
                summary.status = "blocked".into();
                summary.blocked_pair = Some((from, to));
                return Ok(summary);
            }
            PairDecision::Run(from, to) => (from, to),
        };
        let (from, to) = pair;

        // ---- 分类（纯内存，无写入） ----
        let prev_scope = read_scope(db, from)?;
        let curr_scope = read_scope(db, to)?;
        // scope 一致性：任一代缺失/不一致 → 整对跳过（MISSING/DISCOVERED 均不可信）
        let scope_ok = matches!((&prev_scope, &curr_scope), (Some(a), Some(b)) if a.equivalent(b));
        if !scope_ok {
            record_run(
                db,
                volume_id,
                from,
                to,
                run_status::SKIPPED_SCOPE,
                0,
                0,
                Some(&format!(
                    "scope 不一致或缺失（prev={}，curr={}）：范围不同的快照不可做缺失判断",
                    prev_scope.as_ref().map_or("无", |_| "有"),
                    curr_scope.as_ref().map_or("无", |_| "有"),
                )),
                serde_json::json!({ "prev_scope": prev_scope, "curr_scope": curr_scope })
                    .to_string(),
            )?;
            summary.pairs_skipped += 1;
            continue;
        }

        let prev = load_generation(db, volume_id, from)?;
        let curr = load_generation(db, volume_id, to)?;
        let prev_gaps = read_gaps(db, from)?;
        let curr_gaps = read_gaps(db, to)?;
        let missing_set = load_missing_set(db, volume_id)?;
        // 实体映射（尽力关联：观察语义不依赖它）
        let entity_by_path = load_entity_by_path(db, volume_id)?;

        let plan = classify(
            &prev,
            &curr,
            &prev_gaps,
            &curr_gaps,
            &missing_set,
            &entity_by_path,
        );

        // ---- 上限检查（事务前，零副作用） ----
        if plan.events.len() > max_events {
            // 独立状态事务：只写 pending_over_limit 台账行（无任何事件）
            match record_run(
                db,
                volume_id,
                from,
                to,
                run_status::PENDING_OVER_LIMIT,
                0,
                plan.ambiguous_count,
                Some(&format!(
                    "差分事件 {}/{} 超过上限：请人工评估后提高上限重跑或确认放弃",
                    plan.events.len(),
                    max_events
                )),
                serde_json::json!({
                    "from_scan": from, "to_scan": to,
                    "would_emit": plan.events.len(), "max_events": max_events,
                })
                .to_string(),
            ) {
                Ok(()) => {
                    summary.status = "blocked".into();
                    summary.blocked_pair = Some((from, to));
                    summary.ambiguous += plan.ambiguous_count as u64;
                    return Ok(summary);
                }
                Err(e) => {
                    // 独立状态事务失败：pending 不落库（下次触发重算重建），
                    // 如实上报；无部分事件、游标不动
                    summary.status = "failed".into();
                    summary.error = Some(format!("超限且状态记录失败（将自动重试）: {e}"));
                    return Ok(summary);
                }
            }
        }

        // ---- 原子差分事务：台账先行（拿 run_id），事件带 diff_run_id 随后 ----
        let ts = scan_finished_at(db, to)?;
        let tx = ImmediateTx::begin(&db.conn)?;
        tx.conn
            .execute(
                "INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,ambiguous_count,created_at)
                 VALUES(?1,?2,?3,'completed',?4,?5,?6)
                 ON CONFLICT(volume_id,to_scan_id) DO UPDATE SET
                   status='completed', events_count=excluded.events_count,
                   ambiguous_count=excluded.ambiguous_count, created_at=excluded.created_at
                 WHERE scan_diff_runs.status='pending_over_limit'",
                rusqlite::params![volume_id, from, to, plan.events.len(), plan.ambiguous_count, now_ms()],
            )
            .map_err(|e| CoreError::Database(format!("差分台账写入失败: {e}")))?;
        // UPSERT 走 UPDATE 分支时 last_insert_rowid 不更新——显式按唯一键取 id
        let run_id: i64 = tx
            .conn
            .query_row(
                "SELECT id FROM scan_diff_runs WHERE volume_id=?1 AND to_scan_id=?2",
                rusqlite::params![volume_id, to],
                |r| r.get(0),
            )
            .map_err(|e| CoreError::Database(format!("台账 id 查询失败: {e}")))?;
        let mut counts = [0u64; 4]; // discovered/modified/missing/reappeared
        for ev in &plan.events {
            let n = tx.conn.execute(
                "INSERT OR IGNORE INTO file_events(path_id,entity_id,volume_id,rel_path,rel_key,ts,kind,diff_run_id,scan_id,detail)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![
                    ev.path_id,
                    ev.entity_id,
                    volume_id,
                    ev.rel_path,
                    ev.rel_key,
                    ts,
                    ev.kind,
                    run_id,
                    to,
                    ev.detail,
                ],
            )
            .map_err(|e| CoreError::Database(format!("差分事件写入失败: {e}")))?;
            if n > 0 {
                match ev.kind {
                    kinds::DISCOVERED => counts[0] += 1,
                    kinds::MODIFIED => counts[1] += 1,
                    kinds::MISSING => counts[2] += 1,
                    kinds::REAPPEARED => counts[3] += 1,
                    _ => {}
                }
            }
        }
        tx.commit()
            .map_err(|e| CoreError::Database(format!("差分事务提交失败: {e}")))?;

        summary.pairs_completed += 1;
        summary.events_total += plan.events.len() as u64;
        summary.discovered += counts[0];
        summary.modified += counts[1];
        summary.missing += counts[2];
        summary.reappeared += counts[3];
        summary.ambiguous += plan.ambiguous_count as u64;
        summary.missing_suppressed += plan.missing_suppressed;
    }
}

// ---------------------------------------------------------------------------
// 选对（确定性：done 代升序 + 台账连续前缀 + pending 阻塞）
// ---------------------------------------------------------------------------

enum PairDecision {
    None,
    Run(i64, i64),
    Blocked(i64, i64),
}

fn next_pending_pair(db: &Database, volume_id: i64) -> Result<PairDecision, CoreError> {
    let mut gens: Vec<i64> = {
        let mut stmt = db
            .conn
            .prepare(
                "SELECT s.id FROM scans s
                 WHERE s.status='done'
                   AND EXISTS(SELECT 1 FROM paths p WHERE p.scan_id=s.id AND p.volume_id=?1)
                 ORDER BY s.id",
            )
            .map_err(|e| CoreError::Database(format!("扫描代查询失败: {e}")))?;
        let it = stmt
            .query_map([volume_id], |r| r.get::<_, i64>(0))
            .map_err(|e| CoreError::Database(format!("{e}")))?;
        it.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(format!("{e}")))?
    };
    gens.dedup();

    let mut runs: HashMap<i64, String> = HashMap::new(); // to_scan_id → status
    {
        let mut stmt = db
            .conn
            .prepare(
                "SELECT to_scan_id, status FROM scan_diff_runs WHERE volume_id=?1 ORDER BY to_scan_id",
            )
            .map_err(|e| CoreError::Database(format!("台账查询失败: {e}")))?;
        let it = stmt
            .query_map([volume_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| CoreError::Database(format!("{e}")))?;
        for row in it {
            let (to, st) = row.map_err(|e| CoreError::Database(format!("{e}")))?;
            runs.insert(to, st);
        }
    }

    // pending_over_limit 行 = 可重试对：返回 Run，由执行层重新分类——
    // 上限已提高 → 正常完成（条件 UPSERT 覆盖 pending 行）；仍超限 → 刷新
    // pending 并 blocked 返回（后续对保持阻塞）。skipped/completed 历史行
    // 一律不重开（append-only）。
    if gens.len() < 2 {
        return Ok(PairDecision::None); // 基线或空：无差分
    }
    for w in gens.windows(2) {
        let (from, to) = (w[0], w[1]);
        match runs.get(&to) {
            None => return Ok(PairDecision::Run(from, to)),
            Some(st) if st == run_status::PENDING_OVER_LIMIT => {
                return Ok(PairDecision::Run(from, to));
            }
            Some(st) if !run_status::is_processed(st) => {
                return Ok(PairDecision::Blocked(from, to));
            }
            Some(_) => continue, // 已处理，看下一对
        }
    }
    Ok(PairDecision::None)
}

// ---------------------------------------------------------------------------
// 代加载 / 缺失集 / 实体映射 / scope / coverage
// ---------------------------------------------------------------------------

/// 节点原始行（id → 父/名/类型/大小/时间）。
type RawNode = (Option<i64>, String, bool, i64, Option<i64>);

/// 加载一个扫描代的全部路径，按逻辑路径键聚合（值 Vec 以支持歧义检测）。
fn load_generation(db: &Database, volume_id: i64, scan_id: i64) -> Result<GenMap, CoreError> {
    let mut nodes: HashMap<i64, RawNode> = HashMap::new();
    {
        let mut stmt = db
            .conn
            .prepare(
                "SELECT id, parent_id, name, is_dir, size, mtime FROM paths
                 WHERE scan_id=?1 AND volume_id=?2 AND deleted_at IS NULL",
            )
            .map_err(|e| CoreError::Database(format!("加载扫描代失败: {e}")))?;
        let it = stmt
            .query_map(rusqlite::params![scan_id, volume_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? != 0,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })
            .map_err(|e| CoreError::Database(format!("加载扫描代失败: {e}")))?;
        for row in it {
            let (id, parent, name, is_dir, size, mtime) =
                row.map_err(|e| CoreError::Database(format!("{e}")))?;
            nodes.insert(id, (parent, name, is_dir, size, mtime));
        }
    }
    // rel 重建（记忆化链；根节点 rel=""，子级 '/' 拼接——与扫描入库约定一致）
    let mut memo: HashMap<i64, Vec<String>> = HashMap::new();
    let mut out: GenMap = HashMap::new();
    for &id in nodes.keys() {
        let mut rev: Vec<String> = Vec::new();
        let mut cur = Some(id);
        let mut cached = false;
        while let Some(cid) = cur {
            if let Some(parts) = memo.get(&cid).cloned() {
                let mut base = parts;
                for n in rev.iter().rev() {
                    base.push(n.clone());
                }
                memo.insert(id, base);
                cached = true;
                break;
            }
            if let Some((parent, name, ..)) = nodes.get(&cid) {
                rev.push(name.clone());
                cur = *parent;
            } else {
                break;
            }
        }
        if !cached {
            rev.reverse();
            memo.insert(id, rev);
        }
        if let Some(parts) = memo.get(&id) {
            let rel_path = if parts.len() <= 1 {
                String::new()
            } else {
                parts[1..].join("/")
            };
            if rel_path.is_empty() {
                continue; // 登记根自身不参与差分
            }
            let (is_dir, size, mtime) = match nodes.get(&id) {
                Some((_, _, d, s, m)) => (*d, *s, *m),
                None => continue,
            };
            let key = normalize(&rel_path);
            out.entry(key).or_default().push(NodeInfo {
                path_id: id,
                rel_path,
                is_dir,
                size,
                mtime,
            });
        }
    }
    Ok(out)
}

/// 缺失集推导：每逻辑路径键最新差分事件为 file_missing 的键。
/// 三重限定：kind 白名单 / 歧义排除（同键多原样值，两步查询）/ 仅本卷。
/// 返回 rel_key → missing_since_scan_id（REAPPEARED detail 用）。
fn load_missing_set(db: &Database, volume_id: i64) -> Result<HashMap<String, i64>, CoreError> {
    const KINDS: &str = "('file_discovered','file_modified','file_missing','file_reappeared')";
    // 歧义键：同键出现多个不同原样 rel_path（规范化碰撞）——整体排除
    let mut ambiguous: HashSet<String> = HashSet::new();
    {
        let mut stmt = db
            .conn
            .prepare(&format!(
                "SELECT rel_key FROM file_events
                 WHERE volume_id=?1 AND rel_key IS NOT NULL AND kind IN {KINDS}
                 GROUP BY rel_key HAVING COUNT(DISTINCT rel_path) > 1"
            ))
            .map_err(|e| CoreError::Database(format!("歧义查询失败: {e}")))?;
        let it = stmt
            .query_map([volume_id], |r| r.get::<_, String>(0))
            .map_err(|e| CoreError::Database(format!("{e}")))?;
        for row in it {
            ambiguous.insert(row.map_err(|e| CoreError::Database(format!("{e}")))?);
        }
    }
    // 每键最新差分事件
    let mut stmt = db
        .conn
        .prepare(&format!(
            "SELECT rel_key, kind, scan_id FROM (
                SELECT rel_key, kind, scan_id,
                       ROW_NUMBER() OVER (PARTITION BY rel_key ORDER BY id DESC) AS rn
                FROM file_events
                WHERE volume_id=?1 AND rel_key IS NOT NULL AND kind IN {KINDS}
             ) WHERE rn=1"
        ))
        .map_err(|e| CoreError::Database(format!("缺失集查询失败: {e}")))?;
    let it = stmt
        .query_map([volume_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            ))
        })
        .map_err(|e| CoreError::Database(format!("缺失集查询失败: {e}")))?;
    let mut out = HashMap::new();
    for row in it {
        let (key, kind, since) = row.map_err(|e| CoreError::Database(format!("{e}")))?;
        if ambiguous.contains(&key) {
            continue; // 歧义键不参与缺失集（宁可无研判，不错误合并）
        }
        if kind == kinds::MISSING {
            out.insert(key, since);
        }
    }
    Ok(out)
}

fn load_entity_by_path(db: &Database, volume_id: i64) -> Result<HashMap<i64, i64>, CoreError> {
    let mut stmt = db
        .conn
        .prepare(
            "SELECT current_path_id, id FROM file_entities
             WHERE volume_id=?1 AND current_path_id IS NOT NULL",
        )
        .map_err(|e| CoreError::Database(format!("实体映射查询失败: {e}")))?;
    let it = stmt
        .query_map([volume_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(|e| CoreError::Database(format!("{e}")))?;
    it.collect::<Result<HashMap<_, _>, _>>()
        .map_err(|e| CoreError::Database(format!("{e}")))
}

fn read_scope(db: &Database, scan_id: i64) -> Result<Option<ScanScope>, CoreError> {
    let raw: Option<String> = db
        .conn
        .query_row("SELECT scan_scope FROM scans WHERE id=?1", [scan_id], |r| {
            r.get(0)
        })
        .map_err(|e| CoreError::Database(format!("读取 scope 失败: {e}")))?;
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => {
            Ok(Some(serde_json::from_str(&s).map_err(|e| {
                CoreError::Database(format!("解析 scope 失败: {e}"))
            })?))
        }
    }
}

fn read_gaps(db: &Database, scan_id: i64) -> Result<CoverageGaps, CoreError> {
    let raw: Option<String> = db
        .conn
        .query_row(
            "SELECT coverage_gaps FROM scans WHERE id=?1",
            [scan_id],
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Database(format!("读取覆盖缺口失败: {e}")))?;
    Ok(match raw {
        None => CoverageGaps::default(),
        Some(s) if s.is_empty() => CoverageGaps::default(),
        Some(s) => serde_json::from_str(&s)
            .map_err(|e| CoreError::Database(format!("解析覆盖缺口失败: {e}")))?,
    })
}

fn scan_finished_at(db: &Database, scan_id: i64) -> Result<i64, CoreError> {
    db.conn
        .query_row(
            "SELECT COALESCE(finished_at,0) FROM scans WHERE id=?1",
            [scan_id],
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Database(format!("读取扫描完成时刻失败: {e}")))
}

/// 台账写入（独立状态事务：skipped/pending 都只写一行，无事件）。
#[allow(clippy::too_many_arguments)]
fn record_run(
    db: &Database,
    volume_id: i64,
    from: i64,
    to: i64,
    status: &str,
    events_count: usize,
    ambiguous: usize,
    reason: Option<&str>,
    scope_json: String,
) -> Result<(), CoreError> {
    let tx = ImmediateTx::begin(&db.conn)?;
    tx.conn
        .execute(
            "INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,ambiguous_count,scope_json,skip_reason,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(volume_id,to_scan_id) DO UPDATE SET
               status=excluded.status, events_count=excluded.events_count,
               ambiguous_count=excluded.ambiguous_count, scope_json=excluded.scope_json,
               skip_reason=excluded.skip_reason, created_at=excluded.created_at",
            rusqlite::params![volume_id, from, to, status, events_count as i64, ambiguous as i64, scope_json, reason, now_ms()],
        )
        .map_err(|e| CoreError::Database(format!("台账写入失败: {e}")))?;
    tx.commit()
}

// ---------------------------------------------------------------------------
// 分类（纯内存；歧义/覆盖/缺失集三重判定）
// ---------------------------------------------------------------------------

struct PlannedEvent {
    path_id: i64,
    entity_id: Option<i64>,
    rel_path: String,
    rel_key: String,
    kind: &'static str,
    detail: String,
}

struct Plan {
    events: Vec<PlannedEvent>,
    ambiguous_count: usize,
    missing_suppressed: u64,
}

#[allow(clippy::too_many_arguments)]
fn classify(
    prev: &GenMap,
    curr: &GenMap,
    prev_gaps: &CoverageGaps,
    curr_gaps: &CoverageGaps,
    missing_set: &HashMap<String, i64>,
    entity_by_path: &HashMap<i64, i64>,
) -> Plan {
    let mut events = Vec::new();
    let mut ambiguous = 0usize;
    let mut missing_suppressed = 0u64;

    let (curr_dir_gaps, curr_file_gaps) = curr_gaps.match_sets();
    let (prev_dir_gaps, prev_file_gaps) = prev_gaps.match_sets();

    // 歧义集合：双代任一侧同键多值
    let is_ambiguous =
        |k: &str| prev.get(k).map_or(0, |v| v.len()) > 1 || curr.get(k).map_or(0, |v| v.len()) > 1;

    // S_curr 侧：DISCOVERED / MODIFIED / REAPPEARED
    for (key, curr_nodes) in curr {
        if is_ambiguous(key) {
            ambiguous += 1;
            continue;
        }
        let c = &curr_nodes[0];
        let entity_id = entity_by_path.get(&c.path_id).copied();
        match prev.get(key) {
            Some(prev_nodes) => {
                // 两代均有 → MODIFIED 候选（仅文件；size/mtime 均非 NULL 且变化）
                let p = &prev_nodes[0];
                if !c.is_dir
                    && !p.is_dir
                    && (c.size != p.size || c.mtime != p.mtime)
                    && c.mtime.is_some()
                    && p.mtime.is_some()
                {
                    events.push(PlannedEvent {
                        path_id: c.path_id,
                        entity_id,
                        rel_path: c.rel_path.clone(),
                        rel_key: key.clone(),
                        kind: kinds::MODIFIED,
                        detail: serde_json::json!({
                            "from": { "size": p.size, "mtime": p.mtime },
                            "to": { "size": c.size, "mtime": c.mtime },
                            "note": "观察到的元数据变化，不代表内容变化",
                        })
                        .to_string(),
                    });
                }
            }
            None => {
                // S_prev 无 → DISCOVERED 或 REAPPEARED
                if let Some(since) = missing_set.get(key) {
                    events.push(PlannedEvent {
                        path_id: c.path_id,
                        entity_id,
                        rel_path: c.rel_path.clone(),
                        rel_key: key.clone(),
                        kind: kinds::REAPPEARED,
                        detail: serde_json::json!({
                            "missing_since_scan": since,
                            "note": "此前记录为缺失（待确认）的路径重新被观察到；原因不做推断",
                        })
                        .to_string(),
                    });
                } else {
                    // prev_gap 标记：上一代覆盖缺口可能掩盖了它的存在
                    let prev_gap_covered =
                        pathnorm::covered_by_gaps(key, &prev_dir_gaps, &prev_file_gaps);
                    events.push(PlannedEvent {
                        path_id: c.path_id,
                        entity_id,
                        rel_path: c.rel_path.clone(),
                        rel_key: key.clone(),
                        kind: kinds::DISCOVERED,
                        detail: serde_json::json!({
                            "prev_gap_covered": prev_gap_covered || prev_gaps.truncated,
                            "note": "首次被完整扫描观察到，不代表文件刚刚创建",
                        })
                        .to_string(),
                    });
                }
            }
        }
    }

    // S_prev 侧：MISSING 候选（S_curr 覆盖完整性守门）
    for (key, prev_nodes) in prev {
        if curr.contains_key(key) {
            continue;
        }
        if is_ambiguous(key) {
            ambiguous += 1;
            continue;
        }
        // S_curr 覆盖不完整 → 宁漏勿误
        if curr_gaps.truncated || pathnorm::covered_by_gaps(key, &curr_dir_gaps, &curr_file_gaps) {
            missing_suppressed += 1;
            continue;
        }
        let p = &prev_nodes[0];
        events.push(PlannedEvent {
            path_id: p.path_id,
            entity_id: entity_by_path.get(&p.path_id).copied(), // 尽力解析（上一代节点）
            rel_path: p.rel_path.clone(),
            rel_key: key.clone(),
            kind: kinds::MISSING,
            detail: serde_json::json!({
                "last": { "size": p.size, "mtime": p.mtime },
                "note": "本次有效扫描未再观察到该路径——待确认，不代表已删除",
            })
            .to_string(),
        });
    }
    Plan {
        events,
        ambiguous_count: ambiguous,
        missing_suppressed,
    }
}

// ---------------------------------------------------------------------------
// 测试（Windows 真机：真实 tempdir 多代扫描驱动；scope 手工写入——B1-2 接线前的约定）
// ---------------------------------------------------------------------------
#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use crate::scan_writer::run_scan_to_db;
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use tracenest_core::scan_scope::{CoverageGap, CoverageGaps, ScanScope};
    use tracenest_core::scanner::ScanOptions;

    struct Fx {
        db: Database,
        _dir: tempfile::TempDir,
        root: String,
        volume_id: i64,
        scope: ScanScope,
    }

    impl Fx {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = format!("{}\\", dir.path().to_string_lossy().trim_end_matches('\\'));
            let db = Database::open(":memory:".as_ref()).unwrap();
            let volume_id = db
                .upsert_volume(&root, None, None, None, Some("NTFS"), false)
                .unwrap();
            let scope = ScanScope {
                roots: vec![root.clone()],
                exclude_dir_names: ScanOptions::default().exclude_dir_names,
                follow_symlinks: false,
            };
            Self {
                db,
                _dir: dir,
                root,
                volume_id,
                scope,
            }
        }

        /// 一次真实扫描（done 代）+ 写入 scope；返回 scan_id。
        fn scan(&self) -> i64 {
            let scan = self
                .db
                .begin_scan(&[self.root.clone()], "t", self.db.instance_id())
                .unwrap();
            let summary = run_scan_to_db(
                &self.db,
                scan,
                self.volume_id,
                Path::new(&self.root),
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
            self.set_scope(scan, &self.scope);
            scan
        }

        fn set_scope(&self, scan_id: i64, scope: &ScanScope) {
            self.db
                .conn
                .execute(
                    "UPDATE scans SET scan_scope=?1 WHERE id=?2",
                    rusqlite::params![serde_json::to_string(scope).unwrap(), scan_id],
                )
                .unwrap();
        }

        fn set_gaps(&self, scan_id: i64, gaps: &CoverageGaps) {
            self.db
                .conn
                .execute(
                    "UPDATE scans SET coverage_gaps=?1 WHERE id=?2",
                    rusqlite::params![serde_json::to_string(gaps).unwrap(), scan_id],
                )
                .unwrap();
        }

        fn diff(&self) -> DiffSummary {
            self.diff_with(DEFAULT_MAX_DIFF_EVENTS)
        }

        fn diff_with(&self, max_events: usize) -> DiffSummary {
            run_scan_diff(&self.db, self.volume_id, None, max_events).unwrap()
        }

        fn count_kind(&self, kind: &str) -> i64 {
            self.db
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM file_events WHERE kind=?1",
                    [kind],
                    |r| r.get(0),
                )
                .unwrap()
        }

        fn run_count(&self) -> i64 {
            self.db
                .conn
                .query_row("SELECT COUNT(*) FROM scan_diff_runs", [], |r| r.get(0))
                .unwrap()
        }
    }

    fn write(root: &str, rel: &str, bytes: usize) {
        fs::write(format!("{root}{rel}"), vec![0u8; bytes]).unwrap();
    }

    fn rm(root: &str, rel: &str) {
        fs::remove_file(format!("{root}{rel}")).unwrap();
    }

    #[test]
    fn 基线后四类事件_零事件对_连续多代台账() {
        let fx = Fx::new();
        let g1 = fx.scan(); // 基线（空目录）
        let s0 = fx.diff();
        assert_eq!(s0.status, "completed");
        assert_eq!(s0.pairs_completed, 0, "基线无对");
        assert_eq!(fx.run_count(), 0);

        // g2：新文件 → DISCOVERED
        write(&fx.root, "f1.txt", 10);
        let g2 = fx.scan();
        let s = fx.diff();
        assert_eq!(s.pairs_completed, 1);
        assert_eq!(s.discovered, 1);
        assert_eq!(fx.count_kind(kinds::DISCOVERED), 1);
        assert_eq!(fx.count_kind("file_deleted"), 0, "绝不产生 FILE_DELETED");
        // 零事件对：无变化再扫 → completed events=0
        let g3 = fx.scan();
        let s2 = fx.diff();
        assert_eq!(s2.pairs_completed, 1);
        assert_eq!(s2.events_total, 0, "零事件差分也完成研判");
        assert_eq!(fx.run_count(), 2);
        // 重复触发：无新对，幂等
        let s3 = fx.diff();
        assert_eq!(s3.pairs_completed, 0);
        assert_eq!(fx.run_count(), 2);
        // 连续前缀不变式：台账覆盖全部相邻对
        let runs: Vec<i64> = {
            let mut stmt = fx
                .db
                .conn
                .prepare("SELECT to_scan_id FROM scan_diff_runs ORDER BY to_scan_id")
                .unwrap();
            stmt.query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(runs, vec![g2, g3]);
        let _ = g1;
    }

    #[test]
    fn 缺失与重现_挂点与missing_since口径() {
        let fx = Fx::new();
        write(&fx.root, "f1.txt", 10);
        let _g1 = fx.scan(); // 基线（含 f1）
        let g2 = fx.scan(); // 无变化对
        fx.diff();
        // 删除 → g3 → MISSING（挂 g2 节点，missing_since=g3）
        rm(&fx.root, "f1.txt");
        let g3 = fx.scan();
        let s = fx.diff();
        assert_eq!(s.missing, 1);
        assert_eq!(fx.count_kind(kinds::MISSING), 1);
        assert_eq!(fx.count_kind("file_deleted"), 0, "绝不产生 FILE_DELETED");
        let (ev_path, ev_scan): (i64, i64) = fx
            .db
            .conn
            .query_row(
                "SELECT path_id, scan_id FROM file_events WHERE kind='file_missing'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let g2_node: i64 = fx
            .db
            .conn
            .query_row(
                "SELECT id FROM paths WHERE scan_id=?1 AND name='f1.txt'",
                [g2],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ev_path, g2_node, "MISSING 挂最后存在代的节点");
        assert_eq!(ev_scan, g3, "missing_since_scan = 第一次未观察到的扫描代");
        // 重现 → REAPPEARED，missing_since_scan = g3
        write(&fx.root, "f1.txt", 10);
        let _g4 = fx.scan();
        let s2 = fx.diff();
        assert_eq!(s2.reappeared, 1);
        let since: i64 = fx
            .db
            .conn
            .query_row(
                "SELECT json_extract(detail,'$.missing_since_scan') FROM file_events WHERE kind='file_reappeared'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(since, g3);
        // 连续缺失只报一次
        let _g5 = fx.scan();
        let s3 = fx.diff();
        assert_eq!(s3.missing, 0);
        assert_eq!(fx.count_kind(kinds::MISSING), 1);
    }

    #[test]
    fn scope不一致或缺失_整对跳过() {
        let fx = Fx::new();
        let g1 = fx.scan();
        // 旧代 scope 置空（模拟 v6 前历史代）
        fx.db
            .conn
            .execute("UPDATE scans SET scan_scope=NULL WHERE id=?1", [g1])
            .unwrap();
        let g2 = fx.scan();
        let s = fx.diff();
        assert_eq!(s.pairs_skipped, 1, "scope 缺失 → 整对跳过");
        assert_eq!(s.events_total, 0);
        assert_eq!(s.pairs_completed, 0);
        // 范围不一致：改 g2 的 roots
        let mut other = fx.scope.clone();
        other.roots = vec!["E:\\".into()];
        fx.set_scope(g2, &other);
        fx.db
            .conn
            .execute("DELETE FROM scan_diff_runs", [])
            .unwrap();
        let s2 = fx.diff();
        assert_eq!(s2.pairs_skipped, 1);
        let reason: String = fx
            .db
            .conn
            .query_row(
                "SELECT skip_reason FROM scan_diff_runs WHERE to_scan_id=?1",
                [g2],
                |r| r.get(0),
            )
            .unwrap();
        assert!(reason.contains("scope"), "跳过原因必须可追溯");
    }

    #[test]
    fn 覆盖缺口抑制MISSING_宁漏勿误() {
        let fx = Fx::new();
        fs::create_dir_all(format!("{}sub", fx.root)).unwrap();
        write(&fx.root, "sub/f1.txt", 10);
        let _g1 = fx.scan();
        // S_curr 覆盖缺口（目录级）+ f1 消失 → MISSING 必须被抑制
        rm(&fx.root, "sub/f1.txt");
        let g2 = fx.scan();
        fx.set_gaps(
            g2,
            &CoverageGaps {
                paths: vec![CoverageGap {
                    rel_path: "sub".into(),
                    kind: "dir".into(),
                    reason: "denied".into(),
                }],
                truncated: false,
            },
        );
        let s = fx.diff();
        assert_eq!(s.missing, 0, "缺口覆盖下的消失不得误报 MISSING");
        assert_eq!(
            s.missing_suppressed, 1,
            "抑制计数可见（宁漏勿误的诚实证据）"
        );
        // 截断：任何 MISSING 都不产生
        write(&fx.root, "f2.txt", 5);
        let _g3 = fx.scan();
        rm(&fx.root, "f2.txt");
        let g4 = fx.scan();
        fx.set_gaps(
            g4,
            &CoverageGaps {
                paths: vec![],
                truncated: true,
            },
        );
        let s2 = fx.diff();
        assert_eq!(s2.missing, 0);
        assert_eq!(s2.missing_suppressed, 1);
        assert_eq!(fx.count_kind(kinds::MISSING), 0);
    }

    #[test]
    fn 大小写重命名_同逻辑路径不拆分() {
        let fx = Fx::new();
        write(&fx.root, "case.txt", 10);
        let _g1 = fx.scan();
        fs::rename(
            format!("{}/case.txt", fx.root.trim_end_matches('\\')),
            format!("{}/CASE.txt", fx.root.trim_end_matches('\\')),
        )
        .unwrap();
        let _g2 = fx.scan();
        let s = fx.diff();
        assert_eq!(s.missing, 0, "大小写变体必须归并为同一逻辑路径");
        assert_eq!(s.discovered, 0, "不得把同一文件当作新文件");
        assert_eq!(
            fx.count_kind(kinds::MISSING) + fx.count_kind(kinds::DISCOVERED),
            0
        );
    }

    #[test]
    fn 歧义隔离_规范化碰撞不合并不同文件() {
        let fx = Fx::new();
        // ß.txt 与 SS.txt 的 to_uppercase 同键（"SS.TXT"）——真实 NTFS 可共存
        write(&fx.root, "\u{00DF}.txt", 3);
        let _g1 = fx.scan(); // 基线含 ß.txt
        write(&fx.root, "SS.txt", 4);
        let _g2 = fx.scan();
        let s = fx.diff();
        assert_eq!(s.events_total, 0, "歧义键不得产生任何事件（防错误合并）");
        assert!(s.ambiguous >= 1, "歧义计数可见");
        // SS.txt 消失 → K 恢复单值 → 恢复正常；绝不把 SS 的消失记到 ß 头上
        rm(&fx.root, "SS.txt");
        let _g3 = fx.scan();
        let s2 = fx.diff();
        assert_eq!(
            s2.missing + s2.discovered + s2.modified + s2.reappeared,
            0,
            "单键恢复后两代均有 ß.txt → 无事件；绝不产生 SS 的 MISSING"
        );
        assert_eq!(fx.count_kind(kinds::MISSING), 0);
    }

    #[test]
    fn 超限阻塞与两种人工恢复路径() {
        let fx = Fx::new();
        let g1 = fx.scan(); // 基线
        write(&fx.root, "a.txt", 1);
        write(&fx.root, "b.txt", 1);
        write(&fx.root, "c.txt", 1);
        let g2 = fx.scan();
        let s = fx.diff_with(2);
        assert_eq!(s.status, "blocked");
        assert_eq!(s.blocked_pair, Some((g1, g2)));
        assert_eq!(fx.count_kind(kinds::DISCOVERED), 0, "超限不得提交部分事件");
        assert_eq!(fx.run_count(), 1, "仅 pending 台账行");
        // 再次触发：仍阻塞（不自动重试、不推进）
        let s2 = fx.diff_with(2);
        assert_eq!(s2.status, "blocked");
        assert_eq!(fx.count_kind(kinds::DISCOVERED), 0);
        // 后续扫描代也被阻塞（连续前缀）
        write(&fx.root, "d.txt", 1);
        let g3 = fx.scan();
        let s3 = fx.diff_with(2);
        assert_eq!(s3.status, "blocked");
        assert_eq!(s3.blocked_pair, Some((g1, g2)), "阻塞点不变");
        // 恢复路径 A：提高上限重跑 → 依序研判 a/b/c/d
        let s4 = run_scan_diff(&fx.db, fx.volume_id, None, 10).unwrap();
        assert_eq!(s4.status, "completed");
        assert_eq!(s4.discovered, 4);
        let ordered: Vec<i64> = {
            let mut stmt = fx
                .db
                .conn
                .prepare("SELECT scan_id FROM file_events WHERE kind='file_discovered' ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            ordered,
            vec![g2, g2, g2, g3],
            "事件插入顺序=扫描代顺序（阻塞保证线性历史）"
        );
        // 恢复路径 B：确认放弃 → 后续恢复（放弃对无事件，诚实缺口）
        let fx2 = Fx::new();
        let _ = fx2.scan();
        write(&fx2.root, "x.txt", 1);
        write(&fx2.root, "y.txt", 1);
        write(&fx2.root, "z.txt", 1);
        let _g2b = fx2.scan();
        let sb = fx2.diff_with(2);
        assert_eq!(sb.status, "blocked");
        fx2.db
            .conn
            .execute(
                "UPDATE scan_diff_runs SET status='skipped_over_limit', skip_reason='人工确认放弃'
                 WHERE status='pending_over_limit'",
                [],
            )
            .unwrap();
        let sb2 = fx2.diff_with(2);
        assert_eq!(sb2.status, "completed");
        assert_eq!(sb2.pairs_skipped, 0, "放弃已在人工恢复时记录");
        assert_eq!(
            fx2.count_kind(kinds::DISCOVERED),
            0,
            "放弃对无事件（诚实缺口）"
        );
    }

    #[test]
    fn 取消_零写入() {
        let fx = Fx::new();
        let _g1 = fx.scan();
        write(&fx.root, "a.txt", 1);
        let _g2 = fx.scan();
        let flag = AtomicBool::new(true);
        let s = run_scan_diff(&fx.db, fx.volume_id, Some(&flag), DEFAULT_MAX_DIFF_EVENTS).unwrap();
        assert_eq!(s.status, "cancelled");
        assert_eq!(fx.run_count(), 0);
        assert_eq!(fx.count_kind(kinds::DISCOVERED), 0);
    }

    #[test]
    fn m3a兼容_差分不触碰身份与来源数据() {
        let fx = Fx::new();
        fx.db.set_identity_collection_enabled(true).unwrap();
        write(&fx.root, "f1.txt", 1);
        let g1 = fx.scan();
        fx.db
            .conn
            .execute(
                "INSERT INTO tools(key,name,vendor,kind,color,builtin) VALUES('codex','Codex','','cli','#111',1)",
                [],
            )
            .unwrap();
        fx.db
            .conn
            .execute(
                "INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior)
                 SELECT id, scan_id, 1, 'verified', 1.0, '{}', 'unknown' FROM paths
                 WHERE scan_id=?1 AND name='f1.txt'",
                [g1],
            )
            .unwrap();
        write(&fx.root, "f2.txt", 2);
        let _g2 = fx.scan();
        let s = fx.diff();
        assert!(s.discovered >= 1);
        let n = |sql: &str| -> i64 { fx.db.conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(n("SELECT COUNT(*) FROM source_confirmations"), 0);
        assert_eq!(n("SELECT COUNT(*) FROM source_candidates"), 0);
        assert_eq!(
            n("SELECT COUNT(*) FROM attributions WHERE source_behavior<>'unknown'"),
            0
        );
        assert_eq!(
            n("SELECT COUNT(*) FROM file_events WHERE kind LIKE 'source_%'"),
            0
        );
    }
}
