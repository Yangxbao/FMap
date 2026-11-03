# TraceNest 竞品/相关项目调研原始笔记（competitors-raw）

> 调研日期：2026-11-03。方法：GitHub 仓库页（webReader 抓取）、api.github.com（元数据）、raw.githubusercontent.com（源码原文）、WebSearch（补充验证）。所有事实均附 URL；未能核实的条目标注【待验证】。
> TraceNest 定位回顾：跨平台、隐私优先、开源的桌面工具，映射并追踪 AI 编码工具（Claude Code / Codex / Cursor / Gemini CLI 等）在多磁盘上创建的文件；支持回顾式扫描、实时监控、按 AI 工具归属磁盘占用、文件↔项目↔会话关联、陌生目录解释；只读、纯本地、免管理员权限、Windows 优先。

---

## ① 五个指定项目逐个深挖

---

### 1. yamadashy/duvis — "Disk Usage Visualizer for both AI and humans"

- 仓库：https://github.com/yamadashy/duvis
- 元数据（api.github.com/repos/yamadashy/duvis，2026-11-03 读取）：stars **6**，forks 0，open issues 10，license **MIT**，created 2026-03-14，pushed 2026-10-31，语言 Rust。
  - 注：WebSearch 显示不同快照下 stars 为 2–5（GitHub security 页 2、VibeLeaderboard 5），属新仓库小波动；以 API 读取值 6 为准。
- 最新 release：**v0.1.8**（releases 页置顶，https://github.com/yamadashy/duvis/releases；v0.1.8 发布提交为 2026-05-25，commits 页 https://github.com/yamadashy/duvis/commits/main 显示 main 分支最近合并为 2026-07-04，pushed_at 2026-10-31 含其他分支推送）。维护状态：**个人业余项目，活跃期约 2026-03 至 2026-07，之后仅依赖自动更新**。

**实际查看过的文件/路径**
- README 全文：https://github.com/yamadashy/duvis
- Cargo.toml：https://raw.githubusercontent.com/yamadashy/duvis/main/Cargo.toml
- src 目录结构（api contents）：https://api.github.com/repos/yamadashy/duvis/contents/src — `classify/ cli/ filter/ render/ scan/ ui/ wire/ entry.rs lib.rs main.rs`（Facade + 子模块拆分，v0.1.7 release note 亦印证）
- releases/commits 页（含 v0.1.0→v0.1.8 全部 changelog）

**(a) 真正解决的问题**
"磁盘被什么占了" 这一个问题同时服务两类消费者：AI agent（结构化 JSON/NDJSON/TOON 输出，可直接进 `jq`/DB/工具循环）和人类（浏览器 treemap/sunburst/list）。亮点是**自动分类**：每个条目归入 `cache / build / log / media / vcs / ide / other`（核心）+ `archive / installer / vm_image / model_cache / backup`（扩展，按需出现）。（证据：README；releases v0.1.0）

**(b) 源码中验证到的已实现功能**
- Rust 并行扫描（rayon，固定深度以下并行，深层回退串行）（README；Cargo.toml deps `rayon 1.12.0`）
- 分类规则表 + **自顶向下继承**：一旦目录被分类为非 other，其内部全部继承（最外层命名祖先优先，`node_modules` 内的 png 归 cache 而非 media）（README Concepts；v0.1.5 note 提到 classify/facade 拆分与 `explain_*` 复用同一规则表）
- `--explain-category`：解释"这个名字为什么是 cache"，目录/文件两种角色分别给出命中的规则（README；v0.1.5）
- 输出格式：默认树、`--json`（meta+tree，wire_version 2）、`--ndjson`（DFS 前序流式）、`--toon`（token 高效，v0.1.8）、`--summary`、`--largest N`（v0.1.4）
- 过滤器：`--category/--type/--min-size/--name(glob)/--changed-within/--changed-before`，AND 组合，仅作用于显示层，总量不受影响（README；v0.1.4）
- Unix 真实磁盘占用 `st_blocks × 512`（稀疏文件如 OrbStack `data.img.raw` 显示真实占用）、硬链接默认 count-once；**Windows 回退 apparent size、不做硬链接去重**（README "How sizes are measured"）
- 浏览器 UI：React + d3 SPA 内嵌二进制（axum/tokio 服务器，默认端口 7515，被占用自动换空闲端口），treemap/sunburst/list、搜索变暗、Copy path（感知 Windows 反斜杠）/Copy JSON（v0.1.5）
- **只读立场贯彻到底**：v0.1.1 专门删除了 CLI/UI 中所有"可回收/Safe to delete"措辞，并在 UI 放一个**禁用的 "Move to trash" 按钮**作为边界标记（releases v0.1.1）
- 工程质量：单元测试（insta/assert_cmd/tempfile dev-deps）、CI、Codecov、crates.io Trusted Publishing（releases v0.1.1）

**(c) 缺失（对 TraceNest 而言）**
- 无实时监控（纯一次性扫描）
- 无持久化、无事件历史/两次扫描对比
- 无项目↔会话↔文件关联
- AI 归属能力仅到"泛分类"：`model_cache` 认识 `.ollama/ .lmstudio/ .huggingface/`（README Extended），**不认识 Claude Code 的 `~/.claude`、Codex、Cursor 等目录**，不能把占用归属到具体 AI 工具
- Windows 真实占用与硬链接未实现（README 自述）
- 无安装包分发（cargo install 为主，README 承诺 Homebrew/binstall "future release"）

**(d) 值得借鉴的设计**
1. **分类词汇表 + 继承规则**（"删除单元"直觉：`rm -rf node_modules` 是一个整体）—— TraceNest 的 AI 目录知识库可直接沿用该表结构并加一层 AI 工具映射。
2. **`--explain-category` 可解释性**：把"为什么归此类"做成一等公民诊断命令——这正是 TraceNest "解释陌生目录"功能的最小可行原型。
3. **AI-friendly 输出契约**：`meta`（wire_version、items_scanned、items_skipped）+ 截断时的 `truncated_count/truncated_size`，让消费方能知道"看到 N/M"；`--help` 按"agent 冷启动唯一文档"标准撰写（v0.1.2）。
4. **只读边界的产品化表达**（禁用按钮 + hover 提示）——与 TraceNest"只读"定位完全同频。
5. 扫描计数与显示过滤分离（totals 反映全树）的语义约定。

**(e) 不应重建**
- TOON 输出（可后置）；浏览器 UI 的 sunburst 等多视图丰富度； crates 发布基建。

**(f) 与 TraceNest 的关键差异**
duvis 回答"磁盘被**什么类型的东西**占了"，TraceNest 回答"磁盘被**哪个 AI 工具/哪个项目/哪次会话**占了，何时变的"。duvis 是 TraceNest 语义层的子集（分类）+ 缺失维度的旁观者。

**(g) 是否值得作为底层技术参考：是**
哲学最接近（read-only、AI-friendly、分类驱动、MIT 可自由取用），分类规则表与 explain 机制可直接移植；Rust 实现的扫描器（scan/ facade + walk/hardlinks/counts/metadata 拆分，v0.1.7）是干净的小型参考实现。

**(h) 技术栈/维护速览**
Rust(clap/serde/rayon/globset/toon-format/axum) + React+d3 UI；MIT；stars 6；last push 2026-10-31；v0.1.8（2026-05）；单人维护（大量 commit 由 yamadashy 与 Claude 协作完成，releases 页可见 `yamadashy and claude`）。

---

### 2. adileo/squirreldisk — Rust 原生跨平台磁盘分析器（sunburst）

- 仓库：https://github.com/adileo/squirreldisk
- 元数据（api.github.com/repos/adileo/squirreldisk，2026-11-03）：stars **2,124**，forks 106，open issues 26，license **AGPL-3.0**，created 2023-01-29，pushed 2026-10-18，语言 Rust。
- 最新 release：**v2.6.1**（2026-10-18，api releases/latest；releases 页 https://github.com/adileo/squirreldisk/releases 显示 v2.6.1 亦有 Fedora 44 崩溃反馈）。维护状态：**活跃，单人主导 + Discord 社区**。

**实际查看过的文件/路径**
- README：https://github.com/adileo/squirreldisk
- Cargo.toml（根）：https://raw.githubusercontent.com/adileo/squirreldisk/main/Cargo.toml — 包名 `squirreldisk` v2.6.1，依赖 `eframe/egui_glow`（gui feature）、`notify`、`sysinfo`、`trash`、`xxhash-rust/md-5/sha1`、`rfd`、`ureq`、`rodio`、`windows-sys`、`objc2*`
- 根目录清单：https://api.github.com/repos/adileo/squirreldisk/contents/ — `src/ assets/ packaging/ scripts/ .github/`（无 React/无 Tauri 目录）
- src 清单：https://api.github.com/repos/adileo/squirreldisk/contents/src — `agent.rs cli.rs delete.rs disks.rs i18n.rs icon.rs ipc.rs main.rs rclone.rs safety.rs settings.rs sound.rs tree.rs update.rs watch.rs` + `dupes/ scan/ sponsor/ ui/`
- scan 清单：…/contents/src/scan — `demo.rs local.rs mod.rs pathtree.rs platform.rs remote.rs`
- **watch.rs 全文**：https://github.com/adileo/squirreldisk/blob/main/src/watch.rs
- **agent.rs 全文**：https://raw.githubusercontent.com/adileo/squirreldisk/main/src/agent.rs

**(a) 真正解决的问题**
最易用的开源"找大文件"工具：DaisyDisk 式动画 sunburst + WinDirStat/WizTree/TreeSize 的开源替代（README 自述）。当前版本（据仓库 og:description）扩展为"看磁盘、SSH 服务器、云存储的占用，并安全清理"。

**(b) 源码中验证到的已实现功能**
- 纯 Rust + egui 桌面应用（Cargo.toml `eframe`/`egui_glow`）。**注意：README 仍写 "Built With Rust + React (Tauri)"，与 2.6.1 源码不符，README 过时【已核实源码为准】**
- 扫描：`src/scan/local.rs`（本地）、`remote.rs`（远程/SSH）、`pathtree.rs`（路径树）、`platform.rs`
- **实时监控（已实现，源码 verify）**：`watch.rs` 63 行，`notify::recommended_watcher`；macOS/Windows 对根递归 watch（注释："FSEvents 和 ReadDirectoryChangesW 对整盘也便宜；inotify 需要每目录一个 watch"），Linux 仅对当前显示目录 watch（`focus()` 上限 256 个）；过滤 `EventKind::Access`；`normalize_event_path` 把 macOS `/System/Volumes/Data` 前缀翻译回扫描树命名空间
- **headless agent 协议（已实现）**：`agent.rs` — `squirreldisk agent` 子命令，stdout 输出制表符分隔协议：`version` / `scan <path>`（`P` 进度 + 前序遍历 `N` 节点记录：depth、kind d/f/s/m/h/l、size、file count、flags、转义名）/ `dupes-list`（`F` 记录 size/mtime/dev/ino/nlink/path）/ `dupes-hash`（stdin 请求→`H <hex>` 或 `X <err>`）。设计为本地 UI 通过 SSH 驱动远端进程；本质是机器可处理接口
- 重复文件（`dupes/` + xxh3/md5/sha1 依赖）、删除（`delete.rs` + `safety.rs` + `trash` crate）、外部磁盘实时检测（`disks.rs` + `sysinfo`）、自更新（`update.rs` + `ureq` + self-replace）、云存储（`rclone.rs`）、声音（rodio）、i18n
- README 列出的产品功能：快速扫描、整盘或选目录、拖拽收集待删项、右键打开文件管理器、自动更新（仅启动时检查）

**(c) 缺失**
- 无任何 AI 工具知识/归属（不知道 `.claude`、`.cursor` 等）
- 无分类词汇表（无 cache/build 标签体系）
- 无项目↔会话关联、无扫描快照对比、无事件历史
- README 自述"当前版本未 100% 稳定""代码仍是 spaghetti，需要大量重构"；二进制未签名（Win/Mac 提示绕过）
- Windows 下未见 MFT 优化（普通目录遍历）【源码未见，待验证——未逐行读 local.rs】

**(d) 值得借鉴的设计**
1. `watch.rs` 的**平台差异 watch 策略**：Windows/macOS 全盘递归、Linux 按需 focus —— 直接适用于 TraceNest 多磁盘监控。
2. `normalize_event_path`：把 OS 报告的路径映射回扫描树命名空间（macOS 数据卷前缀）；TraceNest 在 Windows 上同样需要处理 junction/OneDrive/前缀差异的等价物。
3. **agent.rs 的 TSV headless 协议**：进度记录 + 前序遍历节点流 + hash 服务分离 —— 是 TraceNest CLI/agent 集成模式（"AI 可编排"）的现成先例。
4. `safety.rs` + `trash` crate 的删除安全分层（TraceNest 只读，但可借鉴其对危险操作的表达方式）。

**(e) 不应重建**
- SSH 远程 agent、rclone 云存储、声音、赞助模块、自更新系统。

**(f) 与 TraceNest 的关键差异**
通用消费级"找大文件+清理"工具；AI 维度为零；**AGPL-3.0**（若 TraceNest 借鉴代码需注意许可证传染，建议只读思路不抄实现，或整体接受 AGPL 兼容策略）。

**(g) 是否值得作为底层技术参考：是（限架构与 watch/agent 模式）**
egui 原生 Rust 桌面 + notify + scan/remote 分层是干净的小型参考；但其自述代码质量不稳定，引用要挑文件。

**(h) 技术栈/维护速览**
Rust（egui/notify/sysinfo/trash/xxh3）；AGPL-3.0；2,124 stars；last push 2026-10-18；v2.6.1（2026-10-18）；单人 + 社区，活跃。

---

### 3. windirstat/windirstat — Windows 磁盘分析"金标准"的现代化 2.x 重写

- 仓库：https://github.com/windirstat/windirstat
- 元数据（api.github.com，2026-11-03）：stars **4,464**，forks 279，open issues 34，license **GPL-3.0+**，created 2016-04-04（GitHub 迁移时间），pushed **2026-11-02**（调研当日仍活跃），语言 C++。
- 最新 release：**WinDirStat 2.9.3**（2026-10-11，api releases/latest；2.x 线持续滚动：2.0.1→2.5.x→2.8.8 beta→2.9.x，WebSearch 印证 https://github.com/windirstat/windirstat/releases）。维护状态：**高度活跃，团队维护**（微软商店/winget/choco/scoop 官方分发）。

**实际查看过的文件/路径**
- README：https://github.com/windirstat/windirstat（Major features 全列表、安装矩阵、兼容矩阵 Win7–Win11/Server 2008R2–2025）
- 源码目录：https://api.github.com/repos/windirstat/windirstat/contents/windirstat — `Build/ Controls/ Dialogs/ Pages/ Views/ logos/ res/` + `FinderNtfs.cpp/.h FinderMtp.cpp FinderRemote.cpp RecoveryNtfs/ExFat/Fat.cpp ItemDupe/ItemPerm/ItemSearch/ItemTop.cpp CsvLoader.cpp Filtering.cpp Localization.cpp DarkMode.cpp Tracer.h WinDirStatModel.cpp/.h` 等 67 文件
- Tracer.h：https://raw.githubusercontent.com/windirstat/windirstat/master/windirstat/Tracer.h
- WinDirStatModel.h：https://raw.githubusercontent.com/windirstat/windirstat/master/windirstat/WinDirStatModel.h

**(a) 真正解决的问题**
Windows 上"磁盘哪里满了 + 怎么清"的一体化工具：多盘/文件夹/网络共享/MTP 设备扫描，treemap/sunburst/flame graph 可视化，大文件/重复文件发现，清理动作集成。（README Description）

**(b) 源码/README 验证的已实现功能**
- 多目标联合扫描（多驱动器+多文件夹），pause/resume/refresh；**elevated 时的 NTFS 加速扫描**（README "accelerated local NTFS scanning when elevated"；对应源码 `FinderNtfs.cpp`）
- 文件类型统计、逻辑 vs 物理大小、硬链接、free/unknown space（README）
- 搜索（名称/大小/类型/所有者、正则）、过滤（路径/名称/大小/年龄）（README；`Filtering.cpp` `ItemSearch.cpp`）
- 重复文件：可配置 hash + 云文件（OneDrive 等）保护，稀疏抽样 hash 标注 "Probable duplicates"（README；`ItemDupe.cpp`）
- 权限检查（`ItemPerm.cpp`）、按修改年龄估算存储分层成本
- 扫描结果 **CSV/JSON 保存/加载**；命令行报告导出 `/SaveTo` `/SaveDupesTo` `/SavePermsTo` `/LoadFrom`（README Getting started）
- **已删除文件恢复**：本地 NTFS/exFAT/FAT，需管理员（README；源码 `RecoveryNtfs.cpp` `RecoveryExFat.cpp` `RecoveryFat.cpp` `RecoveryShared.cpp`）
- 清理动作：Windows 磁盘清理、NTFS 压缩、硬链接去重、自定义命令（README）
- 自定义布局/列/颜色/字体、暗色模式、便携设置、翻译、Explorer 集成（README；`Localization.cpp` `DarkMode.cpp`）
- "Watch file-system changes"：README 与第三方描述（https://www.mediaket.net/software/maintenance-tools/windirstat.html）确认该功能存在；**但源码中我检查的 `Tracer.h` 只是 debug 输出、`WinDirStatModel.h` 无 watcher 类，具体 API（ReadDirectoryChangesW/USN）未能定位【待验证】**

**(c) 缺失**
- 无 AI 工具知识/归属、无项目/会话概念、无时间轴事件历史（"watch" 是实时刷新而非历史记录）； GPL 桌面应用形态，无 headless 结构化输出（除报告导出）。

**(d) 值得借鉴的设计**
1. **"提权可选加速"架构**：非管理员是完整可用的基线，管理员只是 NTFS 更快 —— 这正是 TraceNest"免管理员 + Windows 优先"约束的正确实现范式（可作为 TraceNest 的产品话术与架构边界）。
2. 扫描快照的 **CSV/JSON save/load + CLI 导出 flags**：TraceNest 的事件历史/回放需要等价的持久化与脚本化接口。
3. 搜索/过滤/文件类型统计的 UX 成熟度（这是 25 年品类积累，TraceNest 的"分析视图"要以此为及格线）。
4. 云文件（OneDrive）保护、稀疏抽样 hash 等**防御性细节**。

**(e) 不应重建**
- MFT 扫描器、已删除文件恢复（ forensic 级）、重复文件全量 hash 引擎、权限查看器、MTP/网络设备支持、清理执行器（TraceNest 只读，全部排除）。

**(f) 与 TraceNest 的关键差异**
通用 Windows 清理工具，无 AI 语义；**GPL-3.0+ 对 TraceNest 是许可证隔离区**（代码不可直接复用，除非 TraceNest 同为 GPL 兼容）；C++/MFC 技术栈不可移植。

**(g) 是否值得作为底层技术参考：部分（产品基准，不是代码参考）**
值得作为 UX/功能完整度的对标基准和"elevated-optional"架构范式来源；代码层面无复用价值。

**(h) 技术栈/维护速览**
C++（VS2022，`windirstat.sln`）；GPL-3.0+；4,464 stars；pushed 2026-11-02；2.9.3（2026-10-11）；团队活跃维护。

---

### 4. rodrigokiller/diskplot — Windows 优先、免管理员、带 Live 与 Changes 的新秀（与 TraceNest 气质最接近）

- 仓库：https://github.com/rodrigokiller/diskplot
- 元数据（api.github.com，2026-11-03）：stars **0**，forks 0，license **MIT**，created **2026-10-05**，pushed 2026-10-17，语言 TypeScript。
- 最新 release：**Diskplot 0.4.0**（2026-10-17，api releases/latest）。维护状态：**极新、单人、发布节奏快（一个月 0.1.0→0.4.0），尚无社区**。

**实际查看过的文件/路径**
- README 全文：https://github.com/rodrigokiller/diskplot（Why another one / What it does not do / Speed / How it works / Roadmap）
- 源码目录：https://api.github.com/repos/rodrigokiller/diskplot/contents/src — `main/ mobile/ preload/ renderer/ shared/`
- `src/main/`：https://api.github.com/repos/rodrigokiller/diskplot/contents/src/main — `dirworker.ts scanworker.ts snapshots.ts dupworker.ts index.ts`
- **scanworker.ts 全文**：https://raw.githubusercontent.com/rodrigokiller/diskplot/main/src/main/scanworker.ts
- **dirworker.ts**：https://raw.githubusercontent.com/rodrigokiller/diskplot/main/src/main/dirworker.ts
- **snapshots.ts 全文**：https://raw.githubusercontent.com/rodrigokiller/diskplot/main/src/main/snapshots.ts
- **index.ts**：https://raw.githubusercontent.com/rodrigokiller/diskplot/main/src/main/index.ts

**(a) 真正解决的问题**
作者自述：多年来 SpaceSniffer（map）和 WizTree（tree）同天并用，Diskplot 把**两个视图放进一个窗口**，外加他一直想要的东西：Live（扫描后地图跟随磁盘变化，免重扫）、Changes（每次扫描留下快照，重扫后列出"长了什么/新出现什么/消失了什么"）、Clutter 识别、Find、Duplicates。（README "Why another one"）

**(b) 源码验证的已实现功能**
- **免管理员的快速 Windows 扫描**：coordinator 线程 + 每核 worker；Windows 上每个 worker 用 **koffi 调 `GetFileInformationByHandleEx`（FileFullDirectoryInfo）批量列目录**（一次调用返回数百条目及大小，`CreateFileW` 以 `\\?\` 长路径打开；源码 dirworker.ts 原文引用）；其他平台回退 readdir/lstat。性能：63.4 万文件 1.9s、214.9 万文件 10s（v0.1.0，作者机器，README）
- **列式（columnar）内存表**：不是对象树，而是 typed arrays（parent/size/logical/mtime/flags/nameOff）+ 单一 UTF-8 names 缓冲，约 **60 字节/文件**；线程间零拷贝 transferable buffers；名字搜索是对该缓冲的原生子串搜索（scanworker.ts + README "How it works"）
- worker 池：批 24 目录/job、每 worker 2 个 in-flight、池大小 `max(2, min(12, availableParallelism()))`；实时预览节流（"预览不超过扫描时间 1/10"）；取消是"礼貌退出"而非杀线程（scanworker.ts）
- **Live 实时监控**：`fs.watch`（recursive，Windows 上包装 ReadDirectoryChangesW）每窗口一个 watcher；变更记父目录为 dirty，`WATCH_DELAY=900ms` 合并爆发、每轮批量 `WATCH_BATCH=300` 个目录由 dirworker 重列；Windows 溢出时发 `watch:lost`（"Windows 在变更快于上报时放弃"）（index.ts）
- **快照持久化**：`<rootKey(路径小写 SHA-1 前 12 hex)>-<date>.dps`（gzip level 3 typed arrays + 4 字节头长 + JSON 头）+ `.dps.json` sidecar；每根保留 `KEEP_PER_ROOT=8` 份；文件名正则校验防路径穿越（snapshots.ts）——**Changes（增长/新增/消失）的 diff 基于这些快照**（diff 计算在 renderer 层，index.ts 未含）
- Clutter 识别并汇总：`node_modules`、build 输出、包管理器缓存、虚拟环境、temp、浏览器缓存（README）
- 搜索（边打字边搜 + 最小大小/年龄/类型过滤）、重复文件（等长文件按内容比较，按需）、删除仅经确认进回收站、junction/symlink 列出但不跟随、>260 字符路径、不可读目录计入 Unreadable 不中断（README）
- "size on disk"（压缩/稀疏/OneDrive 仅在线文件按真实占用）；**硬链接按名计数一次 → WinSxS 会虚大**（README 明示）；单色 treemap（深浅=占比）；EN/pt-BR 双语
- 实验性 macOS/Linux/Android 构建；Android 走系统回收/应用缓存清理（README）

**(c) 缺失**
- 无 AI 工具归属（Clutter 是**通用**开发者杂物的目录名匹配，不知道这些目录属于哪个 AI 工具/项目）
- 无项目↔会话关联；Changes 只有两个快照的差集，**不是持久事件时间线**
- 硬链接未感知、无 MFT（Roadmap：可选 MFT reader、hardlink awareness、CSV 导出、winget）（README Roadmap）
- 0 star、无社区验证、Windows 之外未经测试

**(d) 值得借鉴的设计（TraceNest 全部五个核心能力都能在这里找到对应原型）**
1. **列式 typed-array 扫描表（60B/文件）+ bulk 目录枚举** —— TraceNest 回顾式扫描的内存/性能蓝本；koffi+`GetFileInformationByHandleEx` 无需管理员即达秒级。
2. **快照文件格式**（gzip+typed arrays+sidecar meta+按根保留 N 份+路径穿越防护）—— TraceNest 事件历史的存储底座原型。
3. **Live 的 dirty-dir 重列模式**（watch 触发 → 记脏目录 → 防抖 → 批量重列 → 增量进表）—— TraceNest 实时监控的成熟节流范式；以及诚实的 `watch:lost` 处理。
4. "Changes"（grew/new/gone）产品化——事件历史的第一个可交付切片。
5. Clutter 词汇表 + 单色 treemap 纪律 + `size on disk` 语义 + 不跟随 junction 策略。

**(e) 不应重建**
- Android 移动端、内容级重复文件比较（可后置）、非 Windows 平台的实验构建。

**(f) 与 TraceNest 的关键差异**
Diskplot 已经做到"通用杂物 + 快照 diff + Live"，但**完全没有 AI 语义层**：它认识 `node_modules` 却不认识 `~/.claude/projects/<project-hash>/*.jsonl` 属于哪个项目哪次会话。TraceNest = Diskplot 的底盘 + AI 目录知识库 + 归属/关联/事件解释。它是最需要盯住、也最适合直接对标的新对手（MIT 兼容任何借鉴方式）。

**(g) 是否值得作为底层技术参考：是（强烈）**
Electron+TS 与"免管理员 Windows 秒级扫描"的组合与 TraceNest Windows 优先完全重合；若 TraceNest 用 TS/Electron，其 src/main 四文件就是近乎可直接演进的骨架；若 TraceNest 用 Rust，其快照/live 范式仍值得逐条移植。MIT、代码小而新、可全文精读。

**(h) 技术栈/维护速览**
Electron + TypeScript + React（canvas squarified treemap；koffi FFI）；MIT；0 stars；created 2026-10-05；0.4.0（2026-10-17）；单人，极早期。

---

### 5. notify-rs/notify — Rust 跨平台文件系统通知库（事实标准）

- 仓库：https://github.com/notify-rs/notify
- 元数据（api.github.com，2026-11-03）：stars **3,487**，forks 306，open issues 84，repo 级 license 字段为空；README 声明：**notify 核心 = CC0 1.0（公有领域）**，notify-types/debouncer-mini/debouncer-full/file-id = MIT OR Apache-2.0（https://github.com/notify-rs/notify#license）。created 2014-12-11，pushed 2026-11-02。
- 最新 release：notify-types **2.2.0**（2026-10-27，api releases/latest）；notify 主 crate **9.0.0-rc.7**（2026-10-28，https://raw.githubusercontent.com/notify-rs/notify/main/notify/CHANGELOG.md）。维护状态：**非常活跃**（9.0 大版本滚动 RC 中）。

**实际查看过的文件/路径**
- README：https://github.com/notify-rs/notify（平台矩阵、MSRV 1.88、使用方列表：alacritty、cargo watch、deno、mdBook、rust-analyzer、watchexec、zed 等）
- 根目录（workspace 成员）：https://api.github.com/repos/notify-rs/notify/contents/ — `notify/ notify-types/ notify-debouncer-full/ notify-debouncer-mini/ file-id/ docs/ examples/`
- notify/src 清单：https://api.github.com/repos/notify-rs/notify/contents/notify/src — `fsevent.rs inotify.rs kqueue.rs poll.rs windows.rs windows/ null.rs walk.rs config.rs error.rs paths.rs …`
- **windows.rs 全文**：https://raw.githubusercontent.com/notify-rs/notify/main/notify/src/windows.rs
- notify/CHANGELOG.md：https://raw.githubusercontent.com/notify-rs/notify/main/notify/CHANGELOG.md

**(a) 真正解决的问题**
用统一 API 封装各 OS 原生文件通知：Linux/Android inotify、macOS FSEvents（默认）或 kqueue、**Windows ReadDirectoryChangesW**、BSD 系 kqueue、全平台 polling 兜底（README Platforms）。

**(b) 源码验证的已实现功能**
- Windows 后端（windows.rs 原文确认）：`ReadDirectoryChangesW` 异步 overlapped + completion routine；**动态解析 `ReadDirectoryChangesExW`**（Win10 1709+ 扩展信息，避免硬 DLL 依赖）；`CreateFileW` 打开监视目录、信号量唤醒、`CancelIo/CloseHandle` 收尾；未 release 的改进包括"空闲 watcher 不再每秒醒 10 次"与扩展信息分类（CHANGELOG Unreleased）
- 多平台后端 + `poll.rs` 轮询兜底；递归监视
- **debouncer 全家桶**：notify-debouncer-mini/full（full 内含 file-id 去重，事件乱序/重命名对修复）+ notify-types 事件类型（2.2.0 新增 `DebouncedEventBatch` 区分路径变更与 rescan 通知）
- file-id crate：跨平台文件身份（volume+file id），可用于跟踪被移动/重命名的文件
- 事件语义文档化与示例（examples/、docs/）

**(c) 缺失（库的固有边界）**
- 不告诉你**是哪个进程**改了文件（Windows 无公开 API 可直接归因进程【README/文档层面】）
- 不提供初始状态快照（须自扫）；极端高频下有缓冲溢出/事件丢失路径（各后端文档）；rename 的 from/to 配对需 debouncer-full 处理

**(d) 值得借鉴的设计**
1. 直接作为 TraceNest 的监控引擎：`RecommendedWatcher` + `notify-debouncer-full`（file-id 去重）+ `file-id` 做跨移动跟踪 —— TraceNest 实时监控层几乎不需要自研底层。
2. `EventKind` 分类学（Create/Modify/Remove/Access）作为 TraceNest 事件模型的底座。
3. Windows 扩展信息（ReadDirectoryChangesExW）的动态加载写法（Win10 1709+ 特性安全启用）。

**(e) 不应重建**
- 任何自写的 ReadDirectoryChangesW/inotify/FSEvents 封装；自研防抖器。

**(f) 与 TraceNest 的关键差异**
它是 library 而非产品：没有 UI、没有 AI 语义、没有扫描聚合。TraceNest 是它的消费方。

**(g) 是否值得作为底层技术参考：是（必选级）**
若 TraceNest 用 Rust，notify 是唯一合理选择（生态压倒性：deno/rust-analyzer/zed 均在用）；若用 TS/Electron，Node 的 `fs.watch`/chokidar 承担同角色，但 notify 的事件语义与 debouncer 设计仍是最佳参照。

**(h) 技术栈/维护速览**
Rust；CC0 / MIT-OR-Apache-2.0（分 crate）；3,487 stars；pushed 2026-11-02；9.0.0-rc.7（2026-10-28）；多人长期维护。

---

## ② 补充项目清单（开放搜索发现）

### 磁盘分析器（闭源产品）

| 产品 | 链接 | 说明（1–3 行） | 技术栈 | 相关度 | 信息状态 |
|---|---|---|---|---|---|
| **WizTree** | https://diskanalyzer.com | 直接读 NTFS **MFT** 实现超快扫描；**MFT 读取需要管理员权限**；非 NTFS/网络盘回退普通 API 遍历。个人免费、商用 $25–$1,800 | 闭源（Windows） | 高（扫描机制对照 + TraceNest"免管理员"卖点证明） | 已验证（diskanalyzer.com EULA/Features；https://diskanalyzer.com/eula ；MFT+admin 见 diskanalyzer.com 对比页 https://diskanalyzer.com/wiztreemac-vs-wiztree 与 FolderSizes 分析 https://www.foldersizes.com/features/wiztree ） |
| **TreeSize Free** | https://www.jam-software.com/treesize/ | 本地 NTFS 用 **MFT** 加速（文档明示"MFT 仅管理员可访问"），否则回退目录遍历；安装时询问是否提权安装 | 闭源（Windows） | 中 | 已验证（https://documentation.help/TreeSize/whatsnew.html ；https://www.jam-software.com/treesize_free/changes.shtml ） |
| **SpaceSniffer** | https://www.uderzo.it/main_products/space_sniffer | 免费便携 treemap 可视化；**标准目录枚举、不用 MFT、无需管理员**（受限目录结果不全） | 闭源（Windows） | 中 | 已验证（WebSearch 多源：SourceForge/FossHub/Wikipedia） |
| **Everything** | https://www.voidtools.com/ | 用 **MFT** 建索引（首扫秒级）+ **USN Change Journal** 保持实时；**管理员权限才能访问 MFT/USN**，否则回退慢速文件夹索引 | 闭源（Windows） | 高（"MFT+USN 实时"的技术对照与无管理员替代方案参照） | 已验证（官方文档 https://www.voidtools.com/es-ve/support/everything/using_everything ；USN 讨论 https://www.voidtools.com/forum/viewtopic.php?t=26 ） |
| DaisyDisk | https://daisydiskapp.com 【链接待验证——未直接访问】 | macOS 付费 sunburst 磁盘分析器（SquirrelDisk README 将其列为对标） | 闭源（macOS） | 低 | 间接引用（SquirrelDisk README） |

### 文件系统监视器

| 项目 | 链接 | 说明 | 技术栈 | 相关度 | 信息状态 |
|---|---|---|---|---|---|
| **facebook/watchman** | https://github.com/facebook/watchman | Meta 的文件监视**服务**（常驻守护进程）：记录变更、查询、触发动作；git `core.fsmonitor`、React Native 使用。13,758 stars，MIT，C++，pushed 2026-11-02 | C++ | 中（重守护进程形态与 TraceNest"无常驻服务"相悖，但其 query/trigger 与 since-clock 思路值得研究） | 已验证（api.github.com + 官网 https://facebook.github.io/watchman ） |
| **emcrisostomo/fswatch** | https://github.com/emcrisostomo/fswatch | 跨平台文件变更监视 CLI（macOS FSEvents/BSD kqueue/Linux inotify/Windows 等多后端 + libfswatch C 库） | C/C++ | 低-中（后端抽象参考） | 已验证（https://emcrisostomo.github.io/fswatch ） |

### AI 编码工具磁盘占用 / 缓存清理（TraceNest 核心赛道的直接证据）

| 项目 | 链接 | 说明 | 技术栈 | 相关度 | 信息状态 |
|---|---|---|---|---|---|
| **ccusage** | https://github.com/ccusage/ccusage | **19.3k stars**；`npx ccusage` 解析本地 `~/.claude/projects` JSONL，按日/月/**session**/项目统计 token 与成本 —— 现存最接近"会话级归属"的产品（token 域而非磁盘占用域），证明了"从 AI 工具本地目录反推会话↔项目"路线可行 | Rust（npm 分发） | **高** | 已验证（api.github.com：stars 19,300、created 2025-05-29、pushed 2026-11-02、license "Other/NOASSERTION"【license 细节待确认】） |
| **garrickz2/claude-code-cleaner** | https://github.com/garrickz2/claude-code-cleaner | 交互 TUI 清理 `~/.claude/` 积累的磁盘（孤儿项目缓存、旧会话数据、debug 日志）。49 stars，MIT，Rust，created 2026-03、pushed 2026-05 | Rust | **高**（同一问题域：AI 工具占用归因；但它是破坏性清理，TraceNest 是只读归因） | 已验证（api.github.com） |
| **IamRamgarhia/AICacheCleaner** | https://github.com/IamRamgarhia/AICacheCleaner | 识别 AI 进程（MCP server、Claude Code 等）并安全清理其存储 | 未验证 | 高（多 AI 工具意识的清理器） | 未验证元数据【待验证】 |
| **gccszs/disk-cleaner** | https://github.com/gccszs/disk-cleaner | Claude Code skill：跨 Win/Linux/macOS 的磁盘分析、垃圾清理与实时监控 | Claude Code skill | 中 | 未验证元数据【待验证】 |
| **thomas-pedersen/cursor-chat-browser** | https://github.com/thomas-pedersen/cursor-chat-browser | 浏览/搜索/导出 Cursor 的 `state.vscdb`（SQLite）聊天历史 —— Cursor 数据膨胀（forum 报告 state.vscdb 可达 50–100GB：https://forum.cursor.com/t/agent-chat-db-state-vscdb-grows-without-bound-50-bubble-rows-per-visible-message-96-gb-and-climbing/170508/9 ）的社区对策。516 stars，TypeScript，pushed 2026-08 | TypeScript | **高**（"解释陌生目录"+会话读取的同类实践；证明 AI 工具本地格式逆向需求真实） | 已验证（api.github.com） |
| anthropics/claude-code #11646 | https://github.com/anthropics/claude-code/issues/11646 | 官方仓库 feature request：内置 `claude clean`（3GB+ debug 日志、每版本 1GB+ 旧 proxy DB）—— 需求侧证据 | issue | 高（需求证明，非竞品） | 已验证（issue 链接经 WebSearch） |
| alfaXphoori/AgentTrack | https://github.com/alfaXphoori/AgentTrack | 终端 AI 活动追踪（GitHub 风格热图、调用/token） | 未验证 | 低-中 | 未验证元数据【待验证】 |

### 项目级文件追踪 / "这个文件属于谁"

| 项目 | 链接 | 说明 | 技术栈 | 相关度 | 信息状态 |
|---|---|---|---|---|---|
| **git-ai-project/git-ai** | https://github.com/git-ai-project/git-ai | **Git 扩展，逐行追踪 AI 生成的代码**（支持 Codex 等，https://usegitai.com/docs/agents/codex ）。2,928 stars，Apache-2.0，Rust，pushed 2026-11-02 | Rust | 中-高（"AI 归属"在版本控制层的平行实现；语义不同层但可互相引流） | 已验证（api.github.com） |
| **voidcosmos/npkill** | https://github.com/voidcosmos/npkill | 扫描并列出所有 `node_modules` 及体积，选择删除。9,496 stars，MIT，TypeScript，pushed 2026-11-02 | TypeScript | 中（单类别、删除型；TraceNest 的 clutter 词汇表的极端特例） | 已验证（api.github.com） |
| **bootandy/dust** | https://github.com/bootandy/dust | Rust 版"更直观的 du"（树状大文件高亮）。12,574 stars，Apache-2.0，pushed 2026-11-01 | Rust | 低-中（CLI 扫描参考） | 已验证（api.github.com） |
| knorq-ai/agent-format | https://github.com/knorq-ai/agent-format | `.agent` 文件格式（AI 生成的可视化工件） | 未验证 | 低 | 未验证元数据【待验证】 |

> 结论：**没有发现任何一个项目同时具备 TraceNest 的组合**（AI 工具归属 + 多盘回顾扫描 + 实时监控 + 项目↔会话↔文件关联 + 事件历史 + 只读免管理员）。最接近的两组：① diskplot（底盘能力，无 AI）② ccusage / claude-code-cleaner / cursor-chat-browser（AI 语义，无磁盘地图、单工具）。

---

## ③ 对比矩阵

| 项目 | 定位 | 平台 | 技术栈 | 许可证 | 维护状态 | 扫描机制 | 可视化 | 实时监控 | AI 归属能力 | 开源情况 |
|---|---|---|---|---|---|---|---|---|---|---|
| **duvis** | AI+人类两用的分类磁盘可视化 | macOS/Linux/Windows | Rust + React/d3 UI | MIT | 单人；0.1.8（2026-05），push 2026-10 | 普通遍历（rayon 并行；Unix st_blocks 真实占用；Win apparent size） | 浏览器 treemap/sunburst/list | ✗ | 泛分类（含 model_cache：.ollama/.huggingface），无具体 AI 工具 | 开源，cargo 安装 |
| **SquirrelDisk** | 消费级"找大文件+清理" | macOS/Linux/Windows | Rust（egui）+ notify | **AGPL-3.0** | 活跃；v2.6.1（2026-10-18） | 普通遍历（credits parallel-disk-usage） | sunburst（+treemap） | ✓（notify：Win/macOS 递归、Linux 按目录；watch.rs） | ✗ | 开源；SSH/rclone 扩展 |
| **WinDirStat 2.x** | Windows 分析+清理全家桶 | Windows | C++（VS2022） | **GPL-3.0+** | 团队高度活跃；2.9.3（2026-10-11） | 普通遍历 + **elevated 时 NTFS 加速**（FinderNtfs.cpp） | treemap/sunburst/flame + 文件类型 + 列表 | ✓（README 声明 watch；源码 API 未定位【待验证】） | ✗ | 开源（Store/winget/choco/scoop） |
| **Diskplot** | Windows 双视图 + Live + Changes | Windows（其他实验性） | Electron + TS + React + koffi | MIT | 极新单人；0.4.0（2026-10-17） | **GetFileInformationByHandleEx 批量枚举，免管理员**，列式 60B/文件 | canvas 单色 treemap + 树同步 | ✓（fs.watch 递归 + dirty-dir 批量重列） | 仅通用 clutter 词汇（node_modules 等），无 AI 工具知识 | 开源（0 star） |
| **notify** | 文件通知 library | Linux/Android/BSD/macOS/Windows（polling 全平台） | Rust | **CC0（core）+ MIT/Apache-2.0（其他 crate）** | 非常活跃；9.0.0-rc.7（2026-10-28） | ✗（仅事件） | ✗ | ✓（ReadDirectoryChangesW/ExW 等；windows.rs 已验证） | ✗ | 开源 library |
| WizTree | 最快 Windows 分析 | Windows | 闭源 | 免费个人/付费商用 | 商业活跃 | **MFT 直读（需管理员）**；非 NTFS 回退 | treemap+树+文件视图 | （随 MFT/USN 概念，未深究【待验证】） | ✗ | 闭源 |
| TreeSize Free | Windows 分析 | Windows | 闭源 | 免费 | 商业活跃 | **MFT（需管理员）**/回退遍历 | 树+treemap | — | ✗ | 闭源 |
| SpaceSniffer | Windows treemap | Windows | 闭源 | 免费 | 低频更新 | 普通遍历（无 MFT、免管理员） | treemap | — | ✗ | 闭源 |
| Everything | Windows 秒级文件搜索 | Windows | 闭源 | 免费个人 | 商业活跃 | **MFT 索引 + USN Journal 实时（需管理员）** | 列表搜索 | ✓（USN） | ✗ | 闭源 |
| watchman | 文件监视服务 | 全平台 | C++ | MIT | Meta 维护 | 事件 + 快照 | ✗（CLI/API 查询） | ✓（守护进程） | ✗ | 开源 |
| ccusage | Claude Code 用量/成本统计 | CLI（本地 JSONL） | Rust(npm) | Other【待验证】 | 极活跃；19.3k stars | 解析 `~/.claude/projects` | 终端表格 | ✗（按需运行） | **会话/项目级归属（仅 Claude Code、仅 token）** | 开源 |
| claude-code-cleaner | `~/.claude` 清理 TUI | Win/Linux/macOS | Rust | MIT | 低频（49 stars） | 定向扫描 ~/.claude | 终端 TUI | ✗ | 归因到 Claude Code 子目录类别 | 开源 |
| npkill | node_modules 清理 | 全平台 | TypeScript | MIT | 活跃；9.5k stars | 遍历找 node_modules | 终端列表 | ✗ | ✗（通用） | 开源 |
| git-ai | AI 代码逐行归属（git 层） | 全平台 | Rust | Apache-2.0 | 活跃；2.9k stars | git hook 捕获 | ✗ | 事件式（hook） | **代码行级 AI 归属（非磁盘）** | 开源 |

---

## ④ TraceNest 差异化机会与"不应重复造轮子"清单

### 差异化机会（按优先级）
1. **AI 工具归属是空白区**：所有调研对象（开源+闭源）都不认识 `~/.claude`、`~/.codex`、`%APPDATA%/Cursor`、`~/.gemini` 等目录的语义；ccusage/claude-code-cleaner 只覆盖单工具且无磁盘地图。建立 **AI 工具目录知识库**（路径规则 + 版本差异 + "解释陌生目录"）+ **磁盘占用按工具归属**，没有现成对手。
2. **项目↔会话↔文件三级关联**：ccusage 证明了从本地 JSONL 反推会话可行且需求巨大（19.3k stars）；把它扩展到"磁盘上任何文件→属于哪个项目哪次会话"是全新能力。Cursor 的 state.vscdb 膨胀 50–100GB 的用户痛点（forum）佐证"AI 数据可解释性"需求。
3. **事件历史/时间线**：diskplot 的 Changes 只是两快照差集；WinDirStat 的 watch 只是实时刷新；**没有任何产品提供持久化的"AI 文件事件时间线"**（何时创建、被哪个会话产生、增长曲线）。这是纯增量。
4. **免管理员 + Windows 优先 + 隐私本地**：WizTree/TreeSize/Everything 的极速都绑定管理员+MFT；diskplot 证明免管理员也能秒级（koffi+`GetFileInformationByHandleEx`）。"免管理员、只读、零遥测"是清晰的产品矛头（对标 WizTree 付费/TreeSize 商用许可/Everything 需要 admin）。
5. **AI-agent 友好输出**：duvis（JSON/NDJSON/TOON）和 SquirrelDisk agent.rs（TSV 协议）提供了范式，但没有一个是面向"AI 工具磁盘治理"的；TraceNest 的 CLI 可以让 Claude Code 等直接查询自己留下的痕迹。
6. **需求证据链**：anthropics/claude-code#11646（`claude clean` 请求）、Claude Code 3GB+ 日志（同 issue）、Cursor 50–100GB state.vscdb（forum）——痛点真实且在增长。

### 不应重复造轮子
1. **文件监控底层**：Rust → 直接用 notify + notify-debouncer-full（+file-id）；TS/Electron → fs.watch/ReadDirectoryChangesW（或 chokidar）。自写 Win32 封装 = 纯浪费。
2. **Windows 批量目录枚举**：照搬 diskplot 的 `GetFileInformationByHandleEx`（FileFullDirectoryInfo）+ koffi/FFI 模式即可，无需 MFT（MFT 需要管理员，与定位冲突）。
3. **treemap 渲染**：squarified 算法 + "像素预算而非文件数预算"渲染（diskplot）、React+d3 组件（duvis 已开源 MIT）都有现成实现。
4. **扫描表内存布局**：列式 typed arrays + 共享 UTF-8 names 缓冲（diskplot，60B/文件）直接采用该设计而非发明新格式。
5. **分类/杂 Pan 词汇表**：duvis 的 cache/build/log/vcs/ide/model_cache 规则表与继承规则（MIT）可直接 fork，再叠加 AI 工具映射层。
6. **MFT 直读、已删除文件恢复、内容级重复文件引擎、SSH/云存储、自更新系统**：全部与 TraceNest 定位（只读、本地、免管理员）相斥，不碰。
7. **快照持久化格式**：diskplot 的 .dps（gzip typed arrays + sidecar + 按根保留 + 防穿越）可作为起点演化，而不是新设计。

---

## ⑤ 未能访问 / 无法验证事项

1. **WebFetch 直连 github.com 多次失败**（ECONNRESET/timeout），改用 webReader（GitHub 页面渲染版）、api.github.com、raw.githubusercontent.com 完成调研；对结论无影响，但 `github.com/.../tree/...` 目录页未能直接抓取，源码清单均来自 api.github.com/contents。
2. **raw.githubusercontent.com 抓取 squirreldisk `src/watch.rs` 反复 ECONNRESET**：最终经 https://github.com/adileo/squirreldisk/blob/main/src/watch.rs（网页版）取得全文 —— 已核实，仅取数路径曲折。
3. **WinDirStat "Watch file-system changes" 的具体实现 API 未在源码中定位**（检查的 `Tracer.h` 是 debug 日志、`WinDirStatModel.h` 无 watcher）【待验证】；功能存在性由 README + 第三方页面（mediaket.net）佐证。
4. **SquirrelDisk README 与源码不一致**：README 写 "Rust + React (Tauri)"，但 v2.6.1 根 Cargo.toml 显示纯 Rust + eframe/egui、仓库根无 React/Tauri 目录 —— 以源码为准（README 过时）。
5. **SquirrelDisk `src/scan/local.rs` 未逐行阅读**，其 Windows 扫描是否有特殊优化未验证【待验证】。
6. **duvis stars 数在不同快照中为 2–5（WebSearch）vs 6（API 2026-11-03）**；新仓库波动，以 API 读值为准。
7. **notify repo 级 license 字段为 null**（API）；许可证结论取自 README 的分 crate 声明（notify=CC0 1.0，其余 MIT OR Apache-2.0）。
8. **以下补充项目的元数据未经 API 验证**【待验证】：IamRamgarhia/AICacheCleaner、gccszs/disk-cleaner、alfaXphoori/AgentTrack、knorq-ai/agent-format。
9. **ccusage 的许可证 SPDX 为 "NOASSERTION/Other"**（API），具体条款未逐字核对【待验证】。
10. **DaisyDisk 官网未直接访问**（仅作为 SquirrelDisk README 中的对标对象被引用）；daisydiskapp.com 链接为常识推断【待验证】。
11. **WizTree/TreeSize/Everything 的"需管理员读 MFT"结论**来自官方文档/官方对比页/权威第三方（diskanalyzer.com、jam-software 文档、voidtools 文档），未在本机实测提权行为。
12. **duvis main 分支 commits 页最新为 2026-07-04，但 pushed_at=2026-10-31**：差异可能来自非 main 分支或 release 工作流推送，未进一步深挖【待验证】。
13. **diskplot "Changes" 的 diff 计算位于 renderer 层**（index.ts/snapshots.ts 中未含 diff 逻辑，为合理推断），未阅读 renderer 源码确认具体实现【待验证】。
