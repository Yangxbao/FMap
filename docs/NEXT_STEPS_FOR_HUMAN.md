# 交接文档：今晚完成了什么，明天你要做什么（NEXT_STEPS_FOR_HUMAN）

> 写给：项目所有者 · 时间：2026-11-03 夜间 → 11-03 晨
> 一句话总结：**FMap 的全部前期研究、设计与原型已完成并通过实测；编码尚未开始；开工前需要你确认 4 个决策。**

---

## 1. 今晚完成了什么

| # | 交付物 | 位置 | 质量说明 |
|---|---|---|---|
| 1 | 竞品研究报告 | [research/COMPETITORS.md](research/COMPETITORS.md) | 5 个指定项目全部**源码级核查**（读了 duvis/squirreldisk/diskplot 关键源文件与 notify 的 windows.rs 全文），另有 20+ 补充项目 |
| 2 | GitHub 项目清单 | [research/GITHUB_PROJECTS.md](research/GITHUB_PROJECTS.md) | 25 个项目，含 stars/许可证/活跃度/相关度 |
| 3 | AI 工具目录研究报告 | [research/AI_TOOLS_DIRECTORIES.md](research/AI_TOOLS_DIRECTORIES.md) | 9 个工具 + 9 类共享缓存；**本机逐一路径验证**；每条带置信标签 |
| 4 | 技术可行性报告 | [research/TECH_FEASIBILITY.md](research/TECH_FEASIBILITY.md) | 10 问逐答，附微软官方文档链接；USN/监控/进程归因边界全部明确 |
| 5 | 创新功能清单 | [research/INNOVATION_IDEAS.md](research/INNOVATION_IDEAS.md) | 14 项功能 + 评分矩阵 + **6 项明确不做** |
| 6 | PRD | [product/PRD.md](product/PRD.md) | 含 10 个功能需求规格与非功能验收指标 |
| 7 | ROADMAP | [product/ROADMAP.md](product/ROADMAP.md) | V0.1→V1.0 四阶段，含排除项 |
| 8 | 系统设计 | [architecture/SYSTEM_DESIGN.md](architecture/SYSTEM_DESIGN.md) | 模块图、6 条 ADR、Tauri IPC 命令清单、目录结构 |
| 9 | 数据库设计 | [architecture/DATABASE.md](architecture/DATABASE.md) | 完整 DDL 草案（9 张表 + 索引 + FTS5） |
| 10 | 安全设计 | [architecture/SECURITY.md](architecture/SECURITY.md) | 威胁模型 + 5 条硬承诺 + 发布前检查单 |
| 11 | 跨平台兼容性 | [architecture/PLATFORM_COMPATIBILITY.md](architecture/PLATFORM_COMPATIBILITY.md) | 三平台能力矩阵 |
| 12 | 许可证审计 | [research/LICENSE_AUDIT.md](research/LICENSE_AUDIT.md) | 竞品许可证已用 API 核实；本项目许可证建议 |
| 13 | 开发计划 | [development/DEVELOPMENT_PLAN.md](development/DEVELOPMENT_PLAN.md) | V0.1 = 6 个里程碑 ≈6.5 周（单人全职口径） |
| 14 | 验收标准 | [development/ACCEPTANCE_CRITERIA.md](development/ACCEPTANCE_CRITERIA.md) | V0.1 共 30+ 条可观察标准，安全项一票否决 |
| 15 | 本机环境检查 | [development/ENVIRONMENT_CHECK.md](development/ENVIRONMENT_CHECK.md) | 只读检查，未安装任何东西 |
| 16 | UI 静态原型 | [prototype/index.html](../prototype/index.html) | 10 个页面，DEMO 数据，**已在浏览器实测**（渲染/下钻/双主题全通过） |
| 17 | 过程原始笔记 | [research/notes/](research/notes/) | 竞品与 AI 目录的原始调研笔记（含全部证据 URL） |

**全程未做的事**（遵守你的禁令）：未删除/移动/重命名任何文件；未扫描你的个人文件内容（只检查了目录存在性）；未读任何聊天记录；未安装任何软件；未改注册表/环境变量；未建任何公开仓库。本机唯一的运行痕迹 = `FMap/` 文档目录 + 一个本地预览服务器（可随时关闭）。

---

## 2. 哪些结论已经验证（可放心依赖）

1. **免管理员也能秒级扫描**：diskplot 源码证实 `GetFileInformationByHandleEx(FileFullDirectoryInfo)` 路径，214.9 万文件 10s，无需提权。
2. **MFT 直读必须管理员**：WizTree/TreeSize/Everything 官方文档证实 → FMap 排除该路线是正确决策。
3. **实时监控无管理员可行**：notify 的 windows.rs 源码核验（ReadDirectoryChangesW + ExW 动态加载）。
4. **USN 双层边界**：卷级枚举需管理员（排除）；单文件 `FSCTL_READ_FILE_USN_DATA` 免管理员可用（微软官方文档）。
5. **进程级归因任何权限都做不到回溯**——产品只能做 `inferred` 级时间相关性推断（已写入产品设计，绝不夸大）。
6. **AI 归属是空白区**：25 个被调研项目中无一认识 `~/.claude`/`~/.codex` 语义 → 差异化成立。
7. **需求真实存在**：ccusage 19.3k stars、Cursor state.vscdb 单日 1GB/总量 125GB 论坛实证、claude-code#11646 官方 issue。
8. **本机目录实况**（[本机已验证]）：Codex/Claude Code/ZCode/豆包/DSH/WorkBuddy 痕迹都在；Cursor/Gemini/OpenCode 未装；pip/npm 缓存存在。
9. **本机开发环境**：Node 24.21 / npm 11.19 / Git 2.55 / WebView2 156 就绪；缺 Rust 工具链与 MSVC Build Tools（安装命令已备好）。
10. **原型可用**：浏览器实测 10 页全部渲染、treemap 下钻与双主题正常。

## 3. 哪些内容只是推测（不要当结论用）

| 事项 | 现状 |
|---|---|
| ZCode 的 rollout/db 内部格式 | 仅本机观察，无文档 |
| WorkBuddy 内部布局 + `.workbuddy` vs `.codebuddy` 双路径疑点 | 第三方文档交叉，待实机采样 |
| DSH 的 `DSH_HOME`/会话路径 | 官方仓库无路径说明，社区源可信度低 |
| 豆包桌面版的完整目录语义 | 本机确认存在，内容特征靠 Electron 惯例推断 |
| Gemini `<project_hash>` 哈希算法 | 官方未公开 |
| OpenCode 的 Windows 数据目录落点 | XDG 映射待实测 |
| Codex 新版 SQLite 表结构 | 存在性已验证，结构未逆向 |
| Windows 长路径/OneDrive placeholder 的实际行为 | 列为 M1/M2 实测项 |

## 4. 哪些技术存在风险（Top 5）

1. **扫描性能**：std::fs 路径若 M1 基准（100 万文件 ≤5min）不达标 → 已备 `GetFileInformationByHandleEx` 优化路径（架构解耦，风险可控）。
2. **事件丢失**：ReadDirectoryChangesW 缓冲溢出会丢事件 → overflow 计数上 UI + 扫描兜底对账。
3. **AI 工具目录漂移**：Codex 已从 JSONL 迁 SQLite，说明各家格式会变 → 规则包版本化 + 双分支探测 + 社区适配器机制。
4. **diskplot 竞争节奏**：月发 4 版且方向趋近 → V0.1 必须尽快把"AI 归属"做实，窗口期有限。
5. **treemap 大数据渲染**：10 万节点级需"可见节点上限+下钻"，canvas 兜底已在架构预留。

## 5. 值得继续研究的竞品（按优先级）

1. **rodrigokiller/diskplot**（MIT，最直接对手，月 4 版）——持续盯它的 Roadmap（MFT reader、hardlink awareness 都在其列）。
2. **ccusage**（19.3k stars）——它的"从本地目录反推会话"实现细节对 F5 有直接参考价值（注意其许可证 SPDX=Other，引用需谨慎）。
3. **duvis**（MIT）——分类规则表可直接 fork。
4. **git-ai**——磁盘层归属与代码行级归属的联动想象空间。

## 6. 明天（今天早上）建议先做什么

1. **通读 [docs/NEXT_STEPS_FOR_HUMAN.md](NEXT_STEPS_FOR_HUMAN.md)（本文件）** → 5 分钟。
2. **打开 [prototype/index.html](../prototype/index.html) 体验原型** → 10 分钟（双击即可，或用我留的本地预览 http://127.0.0.1:8899/ ）。
3. **翻 [COMPETITORS.md](research/COMPETITORS.md) §0 一页结论 + [INNOVATION_IDEAS.md](research/INNOVATION_IDEAS.md) 评分矩阵** → 15 分钟。
4. **回答 §7 的 4 个决策** → 15 分钟。
5. 决策通过后：装环境（2 条 winget 命令 + rustup，约 30–60 分钟，见 [ENVIRONMENT_CHECK.md](development/ENVIRONMENT_CHECK.md)）→ 建正式 GitHub 仓库 → 进入 M0。

## 7. 正式进入 V0.1 前，需要你确认的 4 个决策

| # | 决策 | 我的建议 | 备选 |
|---|---|---|---|
| D1 | **开源许可证** | `MIT OR Apache-2.0` 双许可（Rust 生态惯例） | 单 MIT（更简） |
| D2 | **项目落位** | 本工作区 `FMap/` 直接 `git init`，首个远程仓库由你创建后推送（我不擅自建公开仓库） | 你指定其他盘符/位置 |
| D3 | **V0.1 扫描范围默认值** | 默认勾选非系统盘 + 排除系统目录；C 盘仅扫用户目录 | 全盘默认扫描 |
| D4 | **隐私敏感功能开关（V0.2 的 F4/F5）** | 默认关闭，首次启用时逐项明示"会读取哪些文件" | 默认开启但可在设置关闭 |

> 另有一个可选小项：Webview2/原型用的临时预览服务器（端口 8899）如不再需要，告诉我一声即可停掉；它只监听 127.0.0.1，无外部暴露。

## 8. 一句话愿景复述

竞品研究证明了一件事：**所有人都在回答"磁盘上有什么"，没有人在回答"AI 把什么放在了你的磁盘上"。** FMap 的窗口期就在这个问句里——V0.1 的唯一使命是把这个问句回答好。
