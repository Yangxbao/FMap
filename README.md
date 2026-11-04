# FMap — AI File Asset Tracker

> 让 AI 在电脑上留下的每一个文件，都有迹可循。

FMap 是一款**跨平台、隐私优先、完全本地运行**的 AI 文件资产追踪工具（Windows 首发）。
它帮助使用 Claude Code、Codex、Cursor、Gemini CLI、ZCode、豆包、DSH 等 AI 工具的用户回答：

- AI 把项目/下载/缓存/模型放到了哪里？
- 每个 AI 工具实际占用了多少空间？
- 这个文件属于哪个工具、哪个项目？
- 过去生成了什么？未来新产生了什么？
- 卸载某个 AI 工具时，哪些文件属于它（避免误删）？

## 当前状态：M0（应用骨架）→ M1（扫描引擎）

| 模块 | 状态 |
|---|---|
| 桌面应用壳（Tauri 2 + React 19 + TypeScript） | ✅ 代码完成，前端已构建/测试/渲染验证；桌面运行已真机验收（M1 收尾） |
| IPC 命令层（4 个只读命令） | ✅ 双侧定义；端到端待桌面运行 |
| SQLite 存储层（WAL + 迁移 + 12 张表） | ✅ 代码完成；**Schema 已实机验证**（node:sqlite）；Rust 测试待工具链 |
| 平台抽象 + Windows 磁盘发现 | ✅ 代码完成（Win32 API，免管理员）；编译待工具链 |
| UI 框架（9 页导航 + 真实系统状态 + 诚实空状态） | ✅ 已实测 |
| 磁盘扫描引擎 | ⏳ M1（计划中） |
| AI 工具识别（目录知识库 + 信号文件 + 置信度） | ⏳ M2（[调研已完成](docs/research/AI_TOOLS_DIRECTORIES.md)） |
| 实时监控 / 历史时间线 / 报告导出 | ⏳ M5 / V0.2 |

> 所有"⏳ 计划中"的功能在 UI 中以明确的开发状态占位展示——**不使用演示数据冒充真实结果**。

## 核心承诺

1. **只读**：不删除、修改、移动、重命名任何用户文件，永不提供清理功能（CI 门禁强制）。
2. **完全本地**：零网络出站；不上传任何文件路径、名称、内容或报告；无账号、无 API Key、无遥测。
3. **无管理员权限**：普通用户权限运行；检测到提权令牌拒绝启动；不使用 MFT 直读等需提权的机制。
4. **隐私功能默认关闭**：读取 AI 会话、聊天记录、文件正文等功能默认关闭、逐项授权。
5. **诚实工程**：每个 AI 归属结论携带置信度（verified / likely / inferred），不夸大能力。

详见 [SECURITY.md](SECURITY.md) 与 [docs/architecture/SECURITY.md](docs/architecture/SECURITY.md)。

## 技术栈

Tauri 2 · Rust（crates: core / database / platform）· React 19 + TypeScript · Tailwind CSS v4 · SQLite (rusqlite, WAL, FTS5) · vitest · Windows 原生 API（免管理员路径）

## 从源码运行

```bash
# 0) 前置（一次性）：Rust stable-msvc + MSVC Build Tools + WebView2（Win10/11 自带）
#    步骤见 docs/M2_FINAL_HANDOFF.md §8 / docs/development/ENVIRONMENT_CHECK.md

# 1) 前端
cd apps/desktop/ui
npm install
npm run build            # 或 npm run dev（配合 tauri dev）

# 2) 桌面应用
cd ../src-tauri
cargo tauri dev          # 需 npx tauri dev（首次会提示安装 @tauri-apps/cli）
```

不装 Rust 也能体验 UI 骨架与测试：

```bash
cd apps/desktop/ui && npm install
npm run build && npm run preview   # 浏览器模式：界面可用，状态栏诚实显示"后端未连接"
npm test                            # 7 个前端测试
node ../../tests/validate-schema.mjs  # 数据库 Schema 真实验证
node ../../tests/check-readonly.mjs   # 只读门禁
```

## 项目结构

```
crates/core        共享类型与纯逻辑（七态扫描状态、错误、格式化）
crates/database    SQLite 层（迁移、卷/扫描 CRUD、统计）
crates/platform    平台抽象（磁盘发现、能力检测、提权检测；Windows 优先）
apps/desktop       Tauri 2 壳 + React 前端
tests/             Schema 权威 DDL + 无需工具链可跑的真实验证脚本
docs/              交接报告 / 研究报告 / PRD / 架构设计 / 开发文档（先读 docs/M2_FINAL_HANDOFF.md）
prototype/         早期静态原型（仅参考，非正式应用）
```

## Roadmap

- **V0.1 · M0** ✅ 应用骨架（本次）
- **V0.1 · M1** 多线程扫描引擎 + 真实磁盘状态（进行中）
- **V0.1 · M2** AI 工具识别（目录知识库 + 信号文件 + 置信度）
- **V0.1 · M3–M5** 查询层 / 全页面接入 / 实时监控（notify）
- **V0.2** 项目指纹跨盘聚合 · 会话↔项目映射 · 卸载安全清单 · 报告导出
- **V0.3** 历史重建（快照差分）· MCP Server 模式 · 隐私审计面板
- **V1.0** macOS / Linux 适配 · 安装器签名 · 正式发布

完整版：[docs/product/ROADMAP.md](docs/product/ROADMAP.md)

## 项目截图

（待 M1 桌面应用可运行后补充真实截图；当前 UI 骨架可用 `npm run preview` 本地查看）

## 文档导航

| 主题 | 文档 |
|---|---|
| 唯一交接入口 | [docs/M2_FINAL_HANDOFF.md](docs/M2_FINAL_HANDOFF.md)（单文件交接制） |
| 产品定义 | [docs/product/PRD.md](docs/product/PRD.md) |
| 差异化研究 | [docs/research/COMPETITORS.md](docs/research/COMPETITORS.md) · [INNOVATION_IDEAS.md](docs/research/INNOVATION_IDEAS.md) |
| 技术边界 | [docs/research/TECH_FEASIBILITY.md](docs/research/TECH_FEASIBILITY.md) |
| 开发交接与历史 | [docs/M2_FINAL_HANDOFF.md](docs/M2_FINAL_HANDOFF.md) · [docs/DEVELOPMENT_LOG.md](docs/DEVELOPMENT_LOG.md)（过程文档已归档于 Git 历史） |

## 许可证

双许可证：[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE) —— 任选其一满足你的使用场景。
第三方依赖许可证审计：[docs/research/LICENSE_AUDIT.md](docs/research/LICENSE_AUDIT.md)。
