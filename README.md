# TraceNest — AI File Asset Tracker

> 让 AI 在电脑上留下的每一个文件，都有迹可循。

TraceNest 是一个**跨平台、隐私优先**的 AI 文件资产地图与追踪工具。它帮助用户回答这些问题：

- AI 工具（Claude Code、Codex、Cursor、Gemini CLI 等）把项目生成到了哪里？
- 哪个 AI 软件占用了多少磁盘空间？
- 这个文件/目录属于哪个 AI 工具、哪个项目？
- 过去生成的文件在哪里？未来新产生的文件如何被记录？
- 卸载某个 AI 工具时，哪些文件属于它（避免误删）？

## 核心原则（不可妥协）

1. **只读**：不删除、不移动、不重命名任何用户文件。
2. **完全本地**：不上传任何文件路径或内容，无遥测，无注册，无 API Key。
3. **无管理员权限**：所有功能在普通用户权限下工作。
4. **开源**：最终发布到 GitHub。
5. **诚实工程**：每个归属结论都带置信度，不夸大操作系统能力。

## 当前状态（2026-11-03 夜间研究）

本目录目前只包含**研究与设计文档**和**静态 UI 原型**，尚未开始正式编码。

| 目录 | 内容 |
|---|---|
| [docs/product/](docs/product/) | PRD、ROADMAP |
| [docs/research/](docs/research/) | 竞品分析、AI 工具目录、技术可行性、创新功能、许可证审计 |
| [docs/research/notes/](docs/research/notes/) | 研究过程原始笔记（含证据链接） |
| [docs/architecture/](docs/architecture/) | 系统设计、数据库、安全、跨平台兼容性 |
| [docs/development/](docs/development/) | 开发计划、验收标准、本机环境检查 |
| [prototype/](prototype/) | 静态 UI 原型（HTML，DEMO 数据） |
| [docs/NEXT_STEPS_FOR_HUMAN.md](docs/NEXT_STEPS_FOR_HUMAN.md) | 交给人类的交接文档（先看这个） |

## 技术栈（规划中）

Tauri 2 · Rust · React · TypeScript · Tailwind CSS · SQLite · D3.js · notify

## 文档阅读顺序建议

1. `docs/NEXT_STEPS_FOR_HUMAN.md` — 今晚成果总览与待确认事项
2. `docs/product/PRD.md` — 产品定义
3. `docs/research/INNOVATION_IDEAS.md` — 差异化方向
4. `docs/research/TECH_FEASIBILITY.md` — 能做什么、不能做什么
5. 其余文档按需阅读
