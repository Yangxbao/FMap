# GitHub 项目清单（GITHUB_PROJECTS）

> 版本：v1.0 · 日期：2026-11-03 · 配套报告：[COMPETITORS.md](COMPETITORS.md)
> 元数据读取日期均为 2026-11-03（api.github.com）；标注【待验证】的行未经 API 核实。

## A. 指定深挖项目（5）

| # | 项目 | Stars | 许可证 | 技术栈 | 最新版本 | 活跃度 | 一句话 | 与 FMap 关系 |
|---|---|---|---|---|---|---|---|---|
| 1 | [yamadashy/duvis](https://github.com/yamadashy/duvis) | 6 | MIT | Rust + React/d3 | v0.1.8 (2026-05) | 中（push 2026-10-31） | AI+人类两用的分类磁盘可视化（JSON/NDJSON/TOON） | 语义子集；分类词汇表+explain 范式可直接借鉴 |
| 2 | [adileo/squirreldisk](https://github.com/adileo/squirreldisk) | 2,124 | **AGPL-3.0** | Rust (egui) | v2.6.1 (2026-10-18) | 活跃 | 消费级 sunburst 找大文件 + SSH/云 | watch 平台策略与 agent 协议可借鉴；⚠️ 许可证隔离 |
| 3 | [windirstat/windirstat](https://github.com/windirstat/windirstat) | 4,464 | **GPL-3.0+** | C++ | 2.9.3 (2026-10-11) | 团队高度活跃 | Windows 磁盘分析金标准（2.x 重写） | UX 及格线 + "提权可选"架构范式；代码零复用 |
| 4 | [rodrigokiller/diskplot](https://github.com/rodrigokiller/diskplot) | 0 | MIT | Electron+TS+koffi | 0.4.0 (2026-10-17) | 极新（月 4 版） | 免管理员秒级扫描 + Live + 快照 Changes | **最直接对手**：底盘范式逐条可移植，但零 AI 语义 |
| 5 | [notify-rs/notify](https://github.com/notify-rs/notify) | 3,487 | CC0（核心）+ MIT OR Apache-2.0（其余 crate） | Rust | 9.0.0-rc.7 (2026-10-28) | 非常活跃 | 跨平台文件通知事实标准 | **必选依赖**（含 debouncer-full + file-id） |

## B. 补充发现：磁盘分析（闭源产品，机制对照）

| 项目 | 链接 | 机制要点 | 相关度 |
|---|---|---|---|
| WizTree | [diskanalyzer.com](https://diskanalyzer.com) | MFT 直读（**需管理员**）；个人免费/商用付费 | 高（反衬 FMap 免管理员卖点） |
| TreeSize Free | [jam-software.com](https://www.jam-software.com/treesize/) | MFT（需管理员）+ 回退遍历 | 中 |
| SpaceSniffer | [uderzo.it](https://www.uderzo.it/main_products/space_sniffer) | 普通遍历、免管理员 treemap | 中 |
| Everything | [voidtools.com](https://www.voidtools.com/) | MFT 索引 + USN Journal 实时（需管理员） | 高（"MFT+USN"技术对照） |
| DaisyDisk | daisydiskapp.com【待验证】 | macOS 付费 sunburst | 低 |

## C. 补充发现：文件监控

| 项目 | Stars | 许可证 | 说明 | 相关度 |
|---|---|---|---|---|
| [facebook/watchman](https://github.com/facebook/watchman) | 13,758 | MIT | Meta 文件监视**服务**（常驻守护，query/trigger/since-clock）；git fsmonitor 在用 | 中（形态相悖，思路可研） |
| [emcrisostomo/fswatch](https://github.com/emcrisostomo/fswatch) | — | GPL | 跨平台 watch CLI + libfswatch | 低-中 |

## D. 补充发现：AI 工具磁盘占用/会话赛道（核心赛道证据）

| 项目 | Stars | 许可证 | 说明 | 相关度 |
|---|---|---|---|---|
| [ccusage/ccusage](https://github.com/ccusage/ccusage) | **19,300** | Other/NOASSERTION【待验证】 | 解析 `~/.claude/projects` JSONL 按 session/项目统计 token 成本 | **高**：证明会话↔项目反推路线可行且需求巨大 |
| [garrickz2/claude-code-cleaner](https://github.com/garrickz2/claude-code-cleaner) | 49 | MIT | TUI 清理 `~/.claude` 磁盘垃圾 | 高：同问题域，破坏性 vs FMap 只读 |
| [thomas-pedersen/cursor-chat-browser](https://github.com/thomas-pedersen/cursor-chat-browser) | 516 | — | 浏览 Cursor `state.vscdb`（SQLite）聊天 | 高：AI 本地格式逆向需求真实（50–100GB 膨胀案例） |
| [anthropics/claude-code #11646](https://github.com/anthropics/claude-code/issues/11646) | issue | — | 官方仓库内建 `claude clean` 请求（3GB+ 日志） | 高：需求侧最硬证据 |
| [IamRamgarhia/AICacheCleaner](https://github.com/IamRamgarhia/AICacheCleaner) | — | — | 多 AI 工具意识清理器【待验证】 | 中-高 |
| [gccszs/disk-cleaner](https://github.com/gccszs/disk-cleaner) | — | — | Claude Code skill 形态的磁盘清理【待验证】 | 中 |
| [alfaXphoori/AgentTrack](https://github.com/alfaXphoori/AgentTrack) | — | — | 终端 AI 活动热图【待验证】 | 低-中 |

## E. 补充发现：归属/清理邻域

| 项目 | Stars | 许可证 | 说明 | 相关度 |
|---|---|---|---|---|
| [git-ai-project/git-ai](https://github.com/git-ai-project/git-ai) | 2,928 | Apache-2.0 | git 层逐行追踪 AI 生成代码（支持 Codex 等） | 中-高：磁盘层归属的平行实现，可互相引流 |
| [voidcosmos/npkill](https://github.com/voidcosmos/npkill) | 9,496 | MIT | 扫描列出所有 node_modules 并删除 | 中：clutter 词汇表极端特例 |
| [bootandy/dust](https://github.com/bootandy/dust) | 12,574 | Apache-2.0 | Rust 版 du（树状大文件） | 低-中：CLI 扫描参考 |
| [knorq-ai/agent-format](https://github.com/knorq-ai/agent-format) | — | — | `.agent` 文件格式【待验证】 | 低 |

## F. 结论速览

- **空白区确认**：没有项目同时具备"AI 工具归属 + 多盘扫描 + 实时监控 + 会话关联 + 事件历史 + 只读免管理员"。
- **两个最近邻**：diskplot（底盘）与 ccusage/claude-code-cleaner/cursor-chat-browser（AI 语义，单工具）。
- **必用/可借鉴**：notify（依赖）、duvis 分类词汇表（MIT fork）、diskplot 扫描/快照范式（MIT 精读）。
- **许可证雷区**：SquirrelDisk（AGPL-3.0）、WinDirStat（GPL-3.0+）——只看思想不抄代码。
