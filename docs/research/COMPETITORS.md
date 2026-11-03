# 竞品研究报告（COMPETITORS）

> 版本：v1.0 · 日期：2026-11-03
> 研究方法：GitHub API 元数据 + README + **源码级核查**（duvis/squirreldisk/diskplot 的关键源文件逐行阅读，notify 的 windows.rs 全文核验）+ 官方文档。
> 原始调研笔记（含全部证据 URL 与源文件清单）：[notes/competitors-raw.md](notes/competitors-raw.md)
> 项目清单（一页表）：[GITHUB_PROJECTS.md](GITHUB_PROJECTS.md)

---

## 0. 一页结论

1. **没有任何现存项目同时具备 TraceNest 的能力组合**（AI 工具归属 + 多盘回顾扫描 + 实时监控 + 项目↔会话↔文件关联 + 事件历史 + 只读免管理员）。
2. 最接近的两组对手：**diskplot**（免管理员快速扫描/Live/快照 diff 的"底盘"，但零 AI 语义）和 **ccusage / claude-code-cleaner / cursor-chat-browser**（AI 语义，但单工具、无磁盘地图）。
3. **AI 文件归属是空白区**：所有被调研项目都不认识 `~/.claude`、`~/.codex`、`%APPDATA%/Cursor` 的语义。
4. 关键技术路径已被验证：**免管理员也能秒级扫描**（diskplot 用 `GetFileInformationByHandleEx` 批量枚举，214.9 万文件 10s）；监控直接用 notify；分类词汇表可直接 fork duvis（MIT）。
5. 需求侧证据充分：anthropics/claude-code#11646（用户请求 `claude clean`，3GB+ 日志）、Cursor state.vscdb 膨胀至 50–100GB 的论坛报告、ccusage 19.3k stars。

---

## 1. 五个指定项目深挖结论

> 每个项目的 8 问全答、查证过的源文件清单与证据链接见 [notes/competitors-raw.md](notes/competitors-raw.md)。此处为决策视角的浓缩。

### 1.1 duvis（yamadashy/duvis）— 哲学最接近，功能是子集

| 维度 | 事实 |
|---|---|
| 定位 | "Disk Usage Visualizer for both AI and humans"：Rust 扫描 + 分类（cache/build/log/vcs/ide/model_cache…）+ 浏览器 treemap + **AI-friendly 结构化输出**（JSON/NDJSON/TOON） |
| 关键数据 | MIT · 6 stars · v0.1.8（2026-05）· 活跃期 2026-03~07 · 单人（yamadashy，repomix 作者） |
| 已验证实现 | rayon 并行扫描；分类规则表+自顶向下继承；`--explain-category` 可解释性命令；`meta` 输出契约（wire_version/truncated_count）；**只读边界产品化**（v0.1.1 专门删除"Safe to delete"措辞，UI 放禁用的 Move-to-trash 按钮作边界标记） |
| 关键缺失 | 无监控、无持久化/历史、无项目↔会话关联；`model_cache` 只认识 .ollama/.lmstudio/.huggingface，**不认识具体 AI 工具目录**；Windows 无真实占用/硬链接去重 |
| 借鉴 | ①分类词汇表+继承规则（MIT 可直接 fork，加 AI 工具映射层）；②`--explain-category` = "解释陌生目录"的最小原型范式；③agent 冷启动友好的 `--help` 与输出契约 |
| 不重建 | TOON 输出（后置）、sunburst 多视图、crates 发布基建 |
| 结论 | **值得作为技术参考（是）**。回答"被什么类型占了"，TraceNest 回答"被哪个 AI/项目/会话占了、何时变的"——语义子集关系 |

### 1.2 SquirrelDisk（adileo/squirreldisk）— 消费级对手，注意 AGPL

| 维度 | 事实 |
|---|---|
| 定位 | 开源 DaisyDisk 式 sunburst"找大文件"，2.6 起扩展 SSH/云存储/清理 |
| 关键数据 | **AGPL-3.0** · 2,124 stars · v2.6.1（2026-10-18）· 活跃单人+社区 |
| 已验证实现 | 纯 Rust + egui（**README 写 Tauri+React 已过时，源码为准**）；`watch.rs`（63 行）：Windows/macOS 根递归 watch、Linux 按需 focus ≤256 目录；`normalize_event_path` 把 OS 路径映射回扫描树；`agent.rs` TSV headless 协议（scan/dupes/hash 服务分离）；删除+safety.rs 分层 |
| 关键缺失 | 零 AI 语义；无分类词汇表；无快照对比/事件历史；作者自述代码"spaghetti"；二进制未签名 |
| 借鉴 | ①**平台差异 watch 策略**（Win/macOS 全盘递归 vs Linux 按需）直接适用于多磁盘监控；②路径命名空间归一化（TraceNest 需处理 junction/OneDrive 的等价物）；③agent 协议模式 |
| 不重建 | SSH/rclone/声音/自更新/赞助模块 |
| 结论 | **值得作为技术参考（是，限 watch/agent 模式）**。⚠️ AGPL 传染：只读思路不抄实现 |

### 1.3 WinDirStat（windirstat/windirstat）— 品类金标准，UX 及格线

| 维度 | 事实 |
|---|---|
| 定位 | Windows 磁盘分析+清理全家桶（2.x 为现代化 C++ 重写） |
| 关键数据 | **GPL-3.0+** · 4,464 stars · 2.9.3（2026-10-11）· 团队高度活跃（pushed 2026-11-02） |
| 已验证实现 | 多目标联合扫描；**"elevated 时 NTFS 加速"= 提权可选架构**（FinderNtfs.cpp）——非管理员是完整基线，管理员只是加速；CSV/JSON 快照 save/load + CLI 导出；重复文件（稀疏抽样 hash + OneDrive 保护）；已删除文件恢复（需管理员） |
| 关键缺失 | 零 AI 语义；"watch" 只是实时刷新而非历史；无 headless 结构化输出 |
| 借鉴 | ①**提权可选**是"免管理员 + Windows 优先"约束的正确架构范式（TraceNest 直接采纳为产品边界）；②快照持久化+CLI 导出接口设计；③搜索/过滤/类型统计的 UX 是 TraceNest 分析视图的及格线 |
| 不重建 | MFT 扫描器、文件恢复、重复文件全量 hash、清理执行器（只读约束全排除） |
| 结论 | **部分参考（产品基准，非代码参考）**。GPL-3.0+ = 许可证隔离区，代码零复用 |

### 1.4 Diskplot（rodrigokiller/diskplot）— 气质最接近的新对手 ⭐重点盯防

| 维度 | 事实 |
|---|---|
| 定位 | "SpaceSniffer + WizTree 同窗并用"：双视图 + **Live**（扫描后地图跟随磁盘变化）+ **Changes**（快照 diff：长了什么/新增/消失）+ Clutter 识别 |
| 关键数据 | MIT · 0 stars · created 2026-10-05 · v0.4.0（2026-10-17）· 极新单人，月发 4 版 |
| 已验证实现（源码级） | **免管理员秒级扫描**：koffi 调 `GetFileInformationByHandleEx(FileFullDirectoryInfo)` 批量列目录 + `\\?\` 长路径（63.4 万文件 1.9s / 214.9 万文件 10s，作者机器）；**列式内存表 ~60B/文件**（typed arrays + 共享 UTF-8 names 缓冲 + 零拷贝 transferable）；Live = fs.watch → dirty-dir 记录 → 900ms 防抖 → 批量 300 目录重列，溢出发 `watch:lost`；**快照格式 .dps**（gzip typed arrays + sidecar + 每根保留 8 份 + 文件名防穿越） |
| 关键缺失 | **零 AI 语义**（认识 node_modules，不认识 ~/.claude/projects/<hash> 属于哪个项目哪次会话）；Changes 只是两快照差集非事件时间线；硬链接未感知；0 star 无社区验证 |
| 借鉴 | 全部五个核心能力都有对应原型：列式扫描表、快照格式、dirty-dir Live 模式、Changes 产品化、Clutter 词汇表。**若 TraceNest 用 Rust，其范式仍逐条可移植** |
| 不重建 | Android 端、内容级重复文件比较（后置） |
| 结论 | **强烈值得参考（MIT、小而新、可全文精读）**。TraceNest = Diskplot 底盘 + AI 目录知识库 + 归属/关联/事件解释。它是最直接的竞品，须持续跟踪 |

### 1.5 notify-rs/notify — 事实标准，必选依赖

| 维度 | 事实 |
|---|---|
| 定位 | Rust 跨平台文件通知库（inotify/FSEvents/ReadDirectoryChangesW/kqueue + polling 兜底） |
| 关键数据 | **notify 核心 CC0-1.0；notify-types/debouncer-*/file-id 为 MIT OR Apache-2.0** · 3,487 stars · 9.0.0-rc.7（2026-10-28）· 多人长期活跃 |
| 已验证实现 | windows.rs 全文核验：ReadDirectoryChangesW 异步 overlapped + **动态解析 ReadDirectoryChangesExW**（Win10 1709+ 特性安全启用）；debouncer-full（file-id 去重、rename 配对）；file-id 跨平台文件身份（volume+file id） |
| 边界 | 不告知是哪个进程改的文件（Windows 无公开 API）；无初始快照；极端高频有溢出丢失路径 |
| 结论 | **必选级依赖**：TraceNest 监控层几乎不需要自研底层。生态压倒性（deno/rust-analyzer/zed/cargo-watch 在用） |

---

## 2. 补充发现（开放搜索，完整表见 GITHUB_PROJECTS.md）

- **闭源产品机制对照**：WizTree / TreeSize / Everything 的极速全部绑定 **MFT 直读（需管理员）**；SpaceSniffer 证明普通遍历+免管理员也能可用。→ TraceNest 的"免管理员、只读、零遥测"因此成为清晰的产品矛头。
- **AI 赛道直接证据**：
  - **ccusage**（19.3k stars）：解析 `~/.claude/projects` JSONL 按 session/项目统计 token——证明"从 AI 工具本地目录反推会话↔项目"路线**可行且需求巨大**（token 域，非磁盘域）。
  - **claude-code-cleaner**（MIT，Rust TUI）：清 `~/.claude` 磁盘垃圾——同一问题域，但它是破坏性清理，TraceNest 是只读归因。
  - **cursor-chat-browser**（516 stars）+ Cursor 论坛报告：state.vscdb 无限增长至 50–100GB——"AI 数据可解释性"痛点真实。
  - **git-ai**（2,928 stars，Apache-2.0）：在 git 层做**代码行级** AI 归属——与 TraceNest 的磁盘层归属是平行互补，可互相引流。
  - anthropics/claude-code **issue #11646**：官方仓库内建 `claude clean` 的用户请求（3GB+ debug 日志/版本）——需求侧最硬的证据。
- 监视服务：facebook/watchman（13.8k stars）为重守护进程形态，与 TraceNest "无常驻服务"相悖，仅研究其 query/since-clock 思路。

---

## 3. 对比矩阵（核心字段）

| 项目 | 许可证 | 免管理员快速扫描 | 实时监控 | 快照/历史 | AI 工具归属 | 会话↔项目 | 只读 |
|---|---|---|---|---|---|---|---|
| duvis | MIT | 普通遍历 | ✗ | ✗ | 泛分类（无具体工具） | ✗ | ✓（哲学） |
| SquirrelDisk | **AGPL-3.0** | 普通遍历 | ✓ notify | ✗ | ✗ | ✗ | ✗（可删除） |
| WinDirStat | **GPL-3.0+** | 提权可选加速 | ✓（API 未定位） | CSV/JSON 手动 | ✗ | ✗ | ✗（清理动作） |
| Diskplot | MIT | ✓（FileFullDirectoryInfo） | ✓ dirty-dir | ✓ .dps 快照 diff | 仅通用 clutter | ✗ | 近只读（回收站删除） |
| notify | CC0/MIT/Apache | ✗ | ✓（库） | ✗ | ✗ | ✗ | ✓ |
| WizTree/TreeSize/Everything | 闭源 | ✓ 但**需管理员** | ✓（USN） | 部分 | ✗ | ✗ | ✗ |
| ccusage | 待核 | ✗ | ✗ | ✗ | **仅 Claude Code（token 域）** | ✓ | ✓ |
| **TraceNest（目标）** | MIT/Apache | ✓（借鉴 diskplot 范式） | ✓ notify+debouncer | ✓ SQLite 事件+快照 | **✓ 全工具知识库+置信度** | **✓** | **✓（硬约束）** |

---

## 4. 差异化机会（按优先级）

1. **AI 工具归属空白区**：建立 AI 目录知识库 + 按工具归属占用 + 置信度分级——无现成对手。
2. **项目↔会话↔文件三级关联**：ccusage 验证了路线，无人把它扩展到"磁盘上任何文件"。
3. **持久化事件时间线**：diskplot 的 Changes 只是快照差集；无人提供"AI 文件事件时间线"。
4. **免管理员 + 隐私本地**：对标 WizTree 付费/TreeSize 商用许可/Everything 需 admin；diskplot 已证明免管理员可秒级。
5. **AI-agent 友好输出/MCP**：duvis 与 squirreldisk agent.rs 提供了范式，无人面向"AI 工具磁盘治理"。

## 5. 不应重复造轮子（直接结论）

1. 监控底层 → `notify` + `notify-debouncer-full` + `file-id`（不自写 Win32 封装）。
2. Windows 批量枚举 → `GetFileInformationByHandleEx(FileFullDirectoryInfo)` 模式（免管理员，勿走 MFT）。
3. 扫描表内存布局 → 列式 typed arrays + 共享 names 缓冲（diskplot 60B/文件设计）。
4. 分类词汇表 → fork duvis（MIT）规则表 + 继承规则，叠加 AI 工具映射层。
5. 快照持久化 → 参照 .dps（gzip+typed arrays+sidecar+按根保留+防穿越）演化为 SQLite 方案。
6. **明确不碰**：MFT 直读、已删除文件恢复、内容级重复文件引擎、SSH/云存储、自更新系统（与只读/本地/免管理员定位相斥）。

## 6. 对 TraceNest 技术栈的影响（研究后修正）

- 原定 Rust + Tauri 2 **维持不变**，但新增两条已验证的性能路径：
  - Rust 侧可用 `windows` crate 直接调 `GetFileInformationByHandleEx`（无需 FFI），实现免管理员批量枚举；
  - 扫描内存布局按列式设计（对 SQLite 批写同样友好）。
- 风险提示：diskplot 迭代极快（月 4 版），若它后续加入 AI 语义层，TraceNest 的窗口期收窄——**V0.1 应尽快把"AI 归属"做实**。

## 7. 未能访问/待验证事项汇总

见 [notes/competitors-raw.md](notes/competitors-raw.md) §⑤（13 条），关键三条：
- WinDirStat "watch" 的具体 API 未在源码定位【待验证】；
- SquirrelDisk Windows 扫描是否特殊优化未逐行验证【待验证】；
- ccusage 许可证 SPDX=Other/NOASSERTION，条款未逐字核对【待验证】。
