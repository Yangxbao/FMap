# M1 遗留问题预检报告（M1_REMAINING_ISSUES_PRECHECK）

> 日期：2026-11-04 · 检查方式：git 状态 + 真实数据库只读查询 + 源码核对

## 0. 项目现状（真实检查结果）

- Git：main 分支 5 提交（…→169c6ff 更名），**工作区干净**（无未提交修改，无需保护）。
- 数据库（%APPDATA%/io.github.tracenest.desktop/tracenest.db，只读检查）：
  - schema_version = **1**；scans 表列 = id,started_at,finished_at,status,roots,files_scanned,dirs_scanned,errors_count,app_version（**无 instance_id / end_note**）
  - scans 表**存在 1 条 status='running' 的孤儿记录**（上次验收性终止进程所致，files_scanned 为 NULL）——问题 2 的真实样本
  - paths = **326,521 条**，与 M1 验收一致，完好
- Rust workspace 17/17、前端 7/7、clippy 0（上次记录）。

## 1. 四项问题真实状态与关键代码

### 问题 1：逐行 INSERT（未修复）
- 位置：`apps/desktop/src-tauri/src/commands.rs` scan_start 的工作线程闭包（约 L200-260）：对每个 WalkEntry 调用 `db.insert_path(...)`，rusqlite 默认 autocommit → **每行一个隐式事务**。
- 另有缺陷：worker 用 `query_latest_running_scan()` 找扫描代——**并发多扫描任务时会错绑**（两任务同时 start 会取同一条）。
- 方案：新增 `run_scan_to_db()`（database crate）统一"遍历→批量事务写→聚合→收尾"；src-tauri 只负责开线程与进度桥接；scan_db_id 由 scan_start 显式传递（消除竞态）。

### 问题 2：孤儿 running（未修复，已有真实样本）
- 位置：`crates/database/src/lib.rs migrate()`（无恢复逻辑）；孤儿样 本 scans.id=1。
- 方案：DB 迁移 v2（scans 加 `instance_id TEXT`、`end_note TEXT`，ALTER 非破坏）；应用启动生成 instance_id，启动恢复 `recover_orphaned_scans()` 把 **instance_id ≠ 当前** 的 running 标记为 failed + end_note（路径记录全保留）；正常关闭时主动把本实例 running 标记 cancelled（边界：多实例并发扫描不支持，见设计文档）。

### 问题 3：can_scan_metadata=false（未修复）
- 位置：`crates/platform/src/lib.rs` windows_impl::capabilities / unsupported::capabilities；前端 Overview"平台能力"卡。
- 方案：Windows 置 true（M1 扫描器已实现并真机验证）；能力模型增加 `can_read_file_contents: false`（区分"读正文"，产品永久只读元数据）；两平台实现 + 前端展示 + 测试同步。

### 问题 4：取消一致性（未完整验证）
- 位置：`crates/core/src/scanner.rs`（walk 内每目录检查 cancel ✓）；`run_scan_to_db`（新写）负责批次边界。
- 方案：集成测试覆盖"批次前/事务中/提交后取消、连续取消、取消后再扫、取消后重启"；实机 UI 取消验证（对真实磁盘做只读扫描 + 中途取消，非破坏性）。

## 2. 数据库影响分析

- 迁移 v2：`ALTER TABLE scans ADD COLUMN instance_id TEXT` + `ADD COLUMN end_note TEXT` —— SQLite ALTER ADD COLUMN 为**非破坏**操作，326,521 条 paths 与既有 scans 行不动；旧库（v1）与新库（全新）两条路径收敛到 v2。
- 无表删除、无列删除、无数据清空。

## 3. 需补充的测试

批量事务回滚（毒行）、取消一致性（四边界）、并发双任务、中文/空格/空目录/深层路径、junction 不跟随、重复扫描、孤儿恢复（真实样本 id=1）、写入耗时（1000/10000/100000）、能力标签。

## 4. 风险

| 风险 | 缓解 |
|---|---|
| 迁移破坏历史数据 | ALTER 非破坏 + 迁移后断言 paths 计数不变 |
| 重构 scan 流程引入回归 | 全量 cargo test + clippy + 实机端到端 |
| 多实例误回收 | instance_id 隔离 + 设计文档明示边界（单实例扫描假设） |
| rustfmt 未安装（minimal profile） | `rustup component add rustfmt`（工具链组件，非系统软件） |
