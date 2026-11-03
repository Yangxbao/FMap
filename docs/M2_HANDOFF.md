# M2 开发交接文档（M2_HANDOFF）

> 2026-11-04 更新：M1 四项遗留已修复并真机验收（见 M1_REMAINING_ISSUES_REPORT.md）。数据库现为 schema v2（scans.instance_id/end_note）；扫描编排已收拢至 scan_writer::run_scan_to_db（批量事务 5000/批，实测 110,206 行/秒）；新测试 28/28。下方原始内容中"未编译"等表述已过时。

> 日期：2026-11-03 · 前置：M1 代码完成（编译验证待工具链）

## 1. 当前状态

- M0（骨架）+ M1（扫描引擎，代码层）完成；前端 7 测试 + 仓库级 2 测试持续全绿。
- Rust 侧累计 14 个单测待 `cargo test`（core 6 + database 4 新 + platform 4）。
- 唯一阻塞：Rust/MSVC 安装（人工，步骤见 M1_HANDOFF §5，不变）。

## 2. 扫描引擎 API（M2 将消费）

```rust
// crates/core/src/scanner.rs
scan_root(root, &ScanOptions, Option<&AtomicBool>, batch_size, on_batch) -> ScanSummary
// ScanOptions::default() 排除 $RECYCLE.BIN 等；链接不跟随
// crates/database
insert_path(..) -> id ；aggregate_dir_sizes(scan_id)；finish_scan(..)
// src-tauri 命令
scan_start(roots) / scan_status(scan_id) / scan_cancel(scan_id)
```

## 3. M1 未完成事项（M2 顺延或并行）

1. cargo 编译 + 14 单测 + clippy（装完工具链第一批命令）
2. 真机扫描验收（对 D 盘/E 盘真实扫描，对账 fixture 与真实计数；GPT 验收问题 2/3 的实证）
3. 扫描性能基准（fixture 10 万文件级 + 真实盘），决定是否切 `GetFileInformationByHandleEx` 优化路径
4. 多线程并行化（当前顺序遍历；架构已解耦）
5. 扫描期间 UI 只读查询的并发压力测试

## 4. M2 推荐任务（按优先级）

1. **规则库落地**：AI_TOOLS_DIRECTORIES.md 的路径表 → `rule_packs/rules` 表种子数据（内置包 v1）
2. **信号文件检测**：CLAUDE.md/AGENTS.md/.cursorrules/GEMINI.md 扫描钩子（扫描器已有 attrs 通道，可扩展 signal 标记）
3. **归属引擎**：目录规则匹配（verified/likely）+ 置信度 + `attributions` 写入
4. **AI 工具识别中心页面**：真实数据渲染（读取 rules + volumes 扫描结果）
5. 完成待验证工具的实机采样（ZCode/WorkBuddy/DSH——用户本机都有痕迹）

## 5. 当前技术风险

| 风险 | 说明 |
|---|---|
| Rust 未编译 | 14 单测 + 命令层未验证；windows crate API 签名可能需微调 |
| 扫描线程写库与 UI 查询并发 | WAL 已开；批事务 2000 行已限；真机验证 |
| 大目录树的 HashMap 内存 | rel_path→id 映射随节点数增长；百万级需评估（基准后处理） |

## 6. M2 新增建议（来自 M1 收尾）

1. 双实例并发扫描的进程存活检测（当前单实例假设）
2. 扫描统计卡自动刷新；应用正常关闭时主动取消在跑任务
3. NSIS 打包验证 FMap 产物名

## 7. 需要用户人工处理

1. 安装 Rust + MSVC（唯一阻塞；命令见 M1_HANDOFF §5）
2. 可选：GitHub 远程仓库创建与首次推送授权
