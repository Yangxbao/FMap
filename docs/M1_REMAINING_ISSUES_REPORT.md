# M1 遗留问题最终报告（M1_REMAINING_ISSUES_REPORT）

> 日期：2026-11-04 · 逐项回答任务书第十一节问题

## 四项遗留问题是否全部修复？
**是，全部修复并真机验证**：
1. **批量事务** ✅：新增 crates/database/src/scan_writer.rs（run_scan_to_db，5000 条/事务，失败整批回滚）；实测 110,206 行/秒（10 万条 0.9s）。
2. **异常退出恢复** ✅：迁移 v2（scans.instance_id/end_note，非破坏 ALTER）+ recover_orphaned_scans + src-tauri setup 接入；真实库三次恢复验证。
3. **can_scan_metadata** ✅：Windows=true（M1 已实现），新增 can_read_file_contents=false（永久），非 Windows 全 false，UI 来自真实后端。
4. **取消一致性** ✅：Rust 集成测试 5 项边界 + 实机 UI 取消（数字精确对账）。

## 修改文件
crates/database/src/{lib.rs, scan_writer.rs(新), scan_writer_tests.rs(新)}；crates/platform/src/lib.rs；apps/desktop/src-tauri/src/{lib.rs, commands.rs}；apps/desktop/ui/src/{lib/ipc.ts, pages/Overview.tsx}；tests/schema.sql（v1 基线不变，v2 走 ALTER）；文档 4 新增 + M2_HANDOFF 更新。

## 数据库是否迁移？
是：schema v1→v2（ALTER ADD COLUMN instance_id/end_note，非破坏）。迁移测试覆盖 v1 旧库路径。

## 历史扫描数据是否保留？
**是**。326,521 条历经迁移、两次强杀、一次并发 BUSY、一次 UI 取消后，paths 总量 508,032 全部为新增累计，无删除/覆盖（每步对账）。

## 批量事务是否真实验证？
是：Rust 基准 1k/10k/100k 三组（135,702/129,810/110,206 行/秒）+ 回滚测试 + 最终行数对账。

## 异常退出恢复是否真实验证？
是：三次真机场景（强杀遗留 + 并发 BUSY 遗留），启动后均正确标 failed + end_note，数据保留。

## can_scan_metadata 是否与真实平台能力一致？
是：Windows=true（有真机验收支撑）、读正文恒 false、非 Windows 全 false；UI 展示来自 get_system_status。

## 完整取消流程是否真实验证？
是：Rust 集成测试（批次前/提交后取消、回滚、再扫描）+ 实机 UI（开始→进行中→取消→cancelled→UI 冻结值与 DB 精确一致）。

## Rust、前端和桌面端测试是否全部执行？
是：Rust 28/28、clippy 0、fmt 通过、前端 7/7+构建、桌面端普通权限启动实测。
（备注：cargo test --workspace 合并运行偶被 Windows Smart App Control 拦截新二进制（os error 4551），以逐 crate 运行等效覆盖并如实记录；SAC 状态=0x1 强制模式，如需彻底解决可由用户手动将其切换为评估模式。）

## 还有哪些未解决问题？
1. 双实例并发扫描的场景边界（instance_id 方案假设单实例，彻底方案需进程存活检测）→ M2
2. 扫描统计卡不自动轮询刷新（数据是快照）→ M2 小改进
3. 应用正常关闭时的主动取消钩子（当前靠启动恢复兜底）→ M2
4. NSIS 打包产物名验证（FMap productName 已配置，未打包验证）

## 是否具备进入 M2 的条件？
**具备**。M1 四项遗留清零，扫描基础可靠性有真实测试与实机验收支撑。
