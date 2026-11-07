# Changelog

## v0.1.0 (2026-11-07)

### 核心功能

- **只读多磁盘扫描引擎**（M1）：七态状态机、批量事务写入（110K 行/秒）、
  取消一致性、异常退出恢复（instance_id）、覆盖缺口收集
- **AI 工具识别与归属**（M2）：内置规则包（15 工具/29 规则）、四级证据归属引擎、
  Verified 目录继承、冲突检测
- **文件身份与来源确认**（M3-A）：卷 GUID + NTFS FileId 主身份（默认关闭）、
  来源确认/修正/撤销（append-only 三层分模型）、路径歧义隔离
- **基础文件事件追踪**（M3-B1）：扫描差分四类事件
  （DISCOVERED/MODIFIED/MISSING/REAPPEARED）、差分台账（原子提交+阻塞+恢复）、
  覆盖缺口抑制 MISSING（宁漏勿误）
- **实时目录监控**（M3-B2）：RDCW 逐目录授权（默认关闭）、原始通知层（零去重）、
  溢出自动对账、gap 生命周期

### 数据库

- Schema v1→v6（阶段事务+幂等补列+崩溃续跑）
- 23 表（meta/volumes/scans/paths/paths_fts/tools/rule_packs/rules/attributions/
  agg_tool_size/events/watch_roots/file_entities/file_events/source_evidence/
  source_candidates/source_confirmations/scan_diff_runs + FTS5 影子表）

### 测试

- Rust 93 测试（core 25 · database 56 · platform 12）
- 前端 vitest 11 测试
- 只读安全门禁 + Schema 验证

### 安全

- 只读门禁（禁止用户文件删除/修改/移动/重命名 API）
- 提权拒绝（管理员运行自动退出）
- 身份采集默认关闭
- 监控默认关闭
