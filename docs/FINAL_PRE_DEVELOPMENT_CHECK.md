# 正式开发前最终检查报告（FINAL_PRE_DEVELOPMENT_CHECK）

> 日期：2026-11-03 · 执行：主会话（无子代理）· 检查方式：实际读取文件 + 只读命令

---

## 1. 昨日研究成果检查结果

### 1.1 完整性

17 项交付物全部在位（docs/product 2、docs/research 6+2 笔记、docs/architecture 4、docs/development 3、prototype 1、NEXT_STEPS 1），路径与交接文档一致。**未重新生成、未覆盖任何已有报告。**

### 1.2 发现的问题与处理

| # | 问题 | 处理 |
|---|---|---|
| P1 | 项目结构：昨日 SYSTEM_DESIGN §6 规划为 `src/ + src-tauri/`；今日指令给出 `apps/desktop + crates/` 参考结构 | **按今日指令为准**（最新决策优先），采用 `apps/desktop/{src-tauri,ui} + crates/{core,database,platform}`。SYSTEM_DESIGN §6 的目录段已在心中标注差异，待 M1 提交时顺手更新该文档（本轮未改动，遵守"不覆盖已有文件"） |
| P2 | 导航清单：原型为 10 页（含"文件来源详情"独立页）；今日指令为 9 项 | **按今日 9 项为准**。"文件来源详情"按 PRD FR-9 的原始定义（详情视图/抽屉）并入文件浏览页，不作为一级导航。原型 10 页保留为参考，不迁入正式应用 |
| P3 | 昨日文档将四项决策列为"待确认" | 今日已获最终决策，**记录于本文档 §3**，并在 README/设置页体现 |
| P4 | AI_TOOLS_DIRECTORIES 中 ZCode/WorkBuddy/DSH/豆包等 6 处【待验证】 | 不阻塞 M0（影响 M2 规则库），已在 M1_HANDOFF 列为 M2 前置调研项 |
| P5 | LICENSE_AUDIT 中 ccusage 许可证 SPDX=Other 未核对 | 不阻塞（本项目不引用其代码），列入发布前检查单 |
| P6 | 原型 index.html 为 DEMO 数据静态页 | 已确认**未迁入正式应用**；正式 UI 为全新代码，空状态全部标注里程碑 |

### 1.3 一致性核查结论

- **数据库设计 ↔ PRD**：一致。FTS5 ↔ PRD 全局搜索；`events` 表 ↔ FR-7 实时监控；`attributions.confidence` ↔ FR-9 置信度依据链；`paths.deleted_at` 软删 ↔ V0.2 历史时间线。Schema 已实机验证（见 §4）。
- **安全设计 ↔ 具体开发要求**：SECURITY.md 的硬承诺已落实为 ①`tests/check-readonly.mjs` CI 门禁（已运行通过）②启动提权自检（`tracenest_platform::is_process_elevated`，提权退出码 87）③Tauri CSP 配置（tauri.conf.json）④应用内设置页如实展示承诺。
- **技术方案内部冲突**：未发现。扫描路线（std::fs 优先 + GetFileInformationByHandleEx 优化）、USN 边界（卷级排除/单文件可选）在 SYSTEM_DESIGN 与 TECH_FEASIBILITY 中口径一致。
- **研究结论错误修正**：本轮检查未发现昨日结论存在需要修正的错误；P2 属产品决策变化而非错误。

## 2. 架构一致性（今日实现的落地情况）

| 昨日确定项 | 今日实现 |
|---|---|
| Tauri 2 + React + TS + Tailwind + SQLite + notify | ✅（notify 按里程碑原则**未引入**——M5 才需要，符合"不要一次性引入所有依赖"） |
| Rust 分层：core/database/platform | ✅ 三个 crate + src-tauri 壳 |
| schema DDL（DATABASE.md） | ✅ 单一权威来源 `tests/schema.sql`，Rust include_str! 与 node:sqlite 验证共用 |
| 无管理员约束 | ✅ 提权自检拒绝启动 + 平台层无任何提权 API |
| 只读约束 | ✅ 门禁测试通过 |

## 3. 四项最终决策的落地

| 决策 | 落地位置 |
|---|---|
| D1 双许可证 MIT OR Apache-2.0 | LICENSE-MIT + LICENSE-APACHE + 各 Cargo.toml `license.workspace` |
| D2 保持当前工作区 | 已在 `FMap/` 下 git init（main 分支，1 个初始提交，无远程） |
| D3 默认扫描所有磁盘 | 设置页默认策略展示 + M1 扫描器设计约束（卷自动发现、权限不足继续） |
| D4 隐私功能默认关闭、逐项授权 | 设置页明示；架构上 M0–M1 无相关代码路径 |

## 4. 环境检查结果（2026-11-03 复检）

| 组件 | 状态 | 说明 |
|---|---|---|
| Node.js v24.21.0 / npm 11.19.0 | ✅ | 前端构建链可用 |
| Git 2.55.0 | ✅ | 身份已配置（赵子龙） |
| WebView2 156.0.4314.58 | ✅ | Tauri 运行时就绪 |
| **Rust (rustc/cargo/rustup)** | ❌ 缺失 | **按指令未安装**。安装方法：winget install Rustlang.Rustup 或 https://rustup.rs （需 stable-msvc 目标） |
| **MSVC Build Tools** | ❌ 缺失 | **按指令未安装**。winget install Microsoft.VisualStudio.2022.BuildTools 后需勾选"使用 C++ 的桌面开发"（含 Windows SDK）；https://visualstudio.microsoft.com/visual-cpp-build-tools/ |
| node:sqlite | ✅ | 已用于 schema 真实验证 |

**影响**：Rust 侧代码（crates + src-tauri）**已编写但未编译**；`cargo test`/`tauri dev` 待工具链安装后执行（M1_HANDOFF §人工事项）。

## 5. 安全风险

1. Rust 代码未经过编译器检验（类型错误风险）→ 已用人工复查 + 简洁代码风格降低；工具链就绪后第一件事是 `cargo clippy`。
2. 前端依赖引入 120 包（esbuild postinstall 被 npm 新版 allowScripts 拦截告警）→ 构建实测正常（vite 7 使用 esbuild wasm/直调路径），后续可用 `npm install-scripts approve esbuild` 规范化。
3. 本地预览服务器（127.0.0.1:8901）为验证用途，监听回环地址，无外部暴露；不需要时可关闭进程。

## 6. 最终开发准备结论

**可以进入 M1。** 前提条件（需人工授权）：安装 Rust 工具链 + MSVC Build Tools（约 30–60 分钟，2–4 GB 磁盘），安装后执行 `M1_HANDOFF.md` §"工具链就绪后立即执行"清单。
