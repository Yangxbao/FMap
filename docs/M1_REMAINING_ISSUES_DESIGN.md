# M1 遗留问题技术设计（M1_REMAINING_ISSUES_DESIGN）

> 日期：2026-11-04 · 前置：M1_REMAINING_ISSUES_PRECHECK.md

## D1. 批量事务写库（问题 1）

**新模块** `crates/database/src/scan_writer.rs`：

```
pub fn run_scan_to_db(
    db: &Database, scan_db_id: i64, volume_id: i64,
    root: &Path, options: &ScanOptions, cancel: Option<&AtomicBool>,
    on_progress: impl FnMut(ScanWriteProgress),
) -> Result<ScanSummary, CoreError>
```

职责收拢：遍历（复用 core::scanner）→ **批量事务写**（默认 5000 条/批，`unchecked_transaction`，批内循环 insert，成功 commit / 失败 rollback 并返回 Err）→ 目录 size 自底向上聚合 → `finish_scan`（含 end_note）。

**批次边界语义**：
- 批 N-1 已 commit、批 N 失败回滚 → 已提交数据完好，扫描结束为 failed（end_note 记录批次号与错误）；**不重试、不跳过该批**（一致性优先，宁少勿错）。
- 取消：取消发生在批次事务中 → 当前批执行完本事务再检查（事务内不中断，保证无半提交）；取消后**已发现未写入的缓冲仍会冲刷提交**（"不丢失已成功发现的记录"），随后状态=cancelled。
- 去重/冲突：paths 表无唯一约束（按设计，扫描代之间允许重复）；同代内重复由 DFS 先序天然避免；约束冲突 → 整批回滚走 failed 路径。

**并发修复**：scan_start 显式 `begin_scan(...) -> db_scan_id` 并存入 ScanJob；worker 直接使用，删除 `query_latest_running_scan()`。

**性能实测口径**：合成 WalkEntry 向量直写测试库，记录 1k/10k/100k 耗时与最终行数。

## D2. 异常退出恢复（问题 2）

**迁移 v2**（SCHEMA_VERSION 1→2）：
```sql
ALTER TABLE scans ADD COLUMN instance_id TEXT;
ALTER TABLE scans ADD COLUMN end_note TEXT;
```
- 兼容性：ALTER ADD COLUMN 非破坏；v1 旧库与新库收敛；迁移后断言 paths 计数不变（测试覆盖）。
- `Database::open` 生成实例标识 `instance_id = format!("{:x}-{:x}", nanos, pid)` 存于结构体。

**恢复函数** `recover_orphaned_scans(&self) -> Result<Vec<i64>>`：
```sql
UPDATE scans SET status='failed',
  end_note='应用在扫描期间退出（启动时恢复）'
WHERE status='running' AND (instance_id IS NULL OR instance_id <> ?1)
RETURNING id;
```
- src-tauri setup 在 open 后调用。paths 记录**全部保留**（软删/聚合不动）。
- **状态语义**：恢复为 failed（现有七态内的诚实表达——任务未完成且不可继续），绝不置 done；end_note 说明原因。
- **边界声明**（无法完美区分的场景）：
  1. 应用正常关闭：本方案下若关闭时有任务在跑 → 留 running → 下次启动标 failed（后续版本可在退出钩子主动标 cancelled，列为 M2 建议）；
  2. 崩溃/强杀：启动恢复覆盖 ✓；
  3. SQLite 写入中进程结束：WAL 保证已提交批完好；未提交批回滚（SQLite 自身保证）✓；
  4. 多实例并发：**M1 假设单实例扫描**——若双实例同时跑，后启动实例会把先启动实例的 running 误标 failed（先实例的 worker 之后 finish_scan 会再翻回终态，最终状态以后写者为准）；边界已在 UI 提示与文档明示，彻底方案（进程存活检测）列 M2。

## D3. 能力标签（问题 3）

`PlatformCapabilities` 增加第四项：`can_read_file_contents: bool`（恒 false，产品只读元数据）。Windows：can_scan_metadata=true（M1 已真机验证）；非 Windows：全 false。前端能力卡增加"读取文件正文（永久关闭）"行，数据全部来自 `get_system_status`。测试：两实现的能力断言更新。

## D4. 取消一致性验证（问题 4）

- **Rust 集成测试**（crates/database/tests/scan_consistency.rs）：合成树（含中文/空格/空目录/深层/大文件/junction），断言：
  1. 不取消全量 → done，计数精确对账；
  2. 第 2 批后置位取消 → status=cancelled，paths 数 = 已提交数 + 缓冲冲刷数（≥ 取消时刻已提交数，≤ 全量），**无半批**；
  3. 毒行批 → 整批回滚、前批完好、状态 failed；
  4. 事务边界：批内失败后同库再写入正常（连接未损坏）。
- **实机验证**：UI 启动 E 盘扫描 → 进行中点击取消 → UI 显示已取消 → DB 断言 status=cancelled 且 paths 计数与 cancel 时刻一致（只读扫描，非破坏）。

## D5. DB 迁移兼容性结论

v1→v2 仅 ADD COLUMN（NULL 填充），**326,521 条 paths 与既有 scans 行不受影响**；回滚兼容：v2 列对 v1 代码不可见但无害。迁移测试：真实库副本上执行迁移前后计数断言。
