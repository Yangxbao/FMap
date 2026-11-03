# 技术可行性报告（TECH_FEASIBILITY）

> 版本：v1.0 · 日期：2026-11-03
> 状态标签：**[已验证-来源]**（官方文档/权威来源/竞品源码核实）· **[本机已验证]**（本机只读检查）· **[待验证]**（M2 里程碑需实测）
> 交叉引用：竞品源码级证据见 [COMPETITORS.md](COMPETITORS.md) 与 [notes/competitors-raw.md](notes/competitors-raw.md)

---

## Q1 能否追踪过去已经存在的文件（回顾式扫描）？

**能，且不需要管理员权限。**

- 普通目录遍历（`std::fs::read_dir` / `FindFirstFileEx`）对当前用户可读的目录完全可用，无需提权。[已验证-来源：Windows API 常规语义]
- **免管理员的快速路径已被竞品源码证实**：diskplot 用 `GetFileInformationByHandleEx(FileFullDirectoryInfo)` 批量列目录（一次调用返回整个目录的条目+大小，`\\?\` 长路径打开），**无需管理员**，实测 214.9 万文件 10 秒。[已验证-来源：diskplot src/main/dirworker.ts 源码 + README]
- **MFT 直读（WizTree/TreeSize/Everything 的极速方案）需要管理员权限**——官方文档明示 MFT 访问需提权。[已验证-来源：diskanalyzer.com、jam-software 文档、voidtools 文档（见 competitors-raw §②）] → TraceNest 定位（无管理员）**排除 MFT 路线**。
- 结论：采用"普通遍历起步 + `GetFileInformationByHandleEx` 批量枚举优化"双路径，见 SYSTEM_DESIGN §2.1。

## Q2 能否追踪未来创建的文件（实时监控）？

**能，不需要管理员权限。**

- Windows：`ReadDirectoryChangesW` 对用户可读目录可用（notify 的 windows.rs 源码全文核验：异步 overlapped + 动态解析 Win10 1709+ 的 `ReadDirectoryChangesExW`）。[已验证-来源：notify-rs/notify src/windows.rs]
- 跨平台：notify 统一封装（inotify / FSEvents / ReadDirectoryChangesW / kqueue / polling 兜底），配 `notify-debouncer-full`（file-id 去重、rename 配对）。[已验证-来源：notify README + 源码结构]
- **已知限制**：①缓冲区溢出时事件丢失（Windows 表现为放弃上报——diskplot 源码中的 `watch:lost` 处理即为此）；②Linux inotify 每目录一个 watch（见 Q6）；③事件不携带进程信息（见 Q4）。[已验证-来源：notify 文档 + diskplot index.ts]

## Q3 NTFS USN Journal 能否用于"无管理员"的辅助建链？

**分两级，边界清晰：**

| 操作 | 权限要求 | TraceNest 可用性 |
|---|---|---|
| `FSCTL_ENUM_USN_DATA`（全量枚举 MFT） | 需要管理员（需以 GENERIC_READ 打开 `\\.\C:` 卷句柄） | ❌ 排除（违反约束） |
| `FSCTL_READ_USN_JOURNAL`（读取变更日志） | 同上，需管理员 | ❌ 排除 |
| **`FSCTL_READ_FILE_USN_DATA`（读单个文件的 USN 记录：FileReferenceNumber + ParentFileReferenceNumber）** | **普通文件句柄即可，无需管理员** | ✅ 可用（可选增强） |

[已验证-来源：[FSCTL_READ_FILE_USN_DATA 官方文档](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_read_file_usn_data)、[MS-FSCC 协议规范](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/d643cdfa-5f4f-4737-a905-4098d4577593)、卷级操作需提权见 [Learn Q&A](https://learn.microsoft.com/en-us/answers/questions/1726443/how-to-call-fsctl-get-ntfs-volume-data-ioctl-from) 与 [change journal operations](https://learn.microsoft.com/en-us/windows/win32/fileio/change-journal-operations)]

- 由此得到的**诚实能力**：对**已知文件**可取其父引用号辅助建立父子链（V0.2+ 可选增强）；把父引用号批量解析回路径（`OpenFileById`）的可行性与权限细节 **[待验证]**（M2 实测项）。
- **不能**：从 USN 重建全盘历史（需管理员 + 数据不可靠）——产品层面不做（ROADMAP V0.3 排除项）。

## Q4 能否判断文件由哪个进程创建？

**实时精确归因：不能（无管理员权限）。回溯归因：任何权限都做不到。**

- ETW 内核文件事件（Microsoft-Windows-Kernel-File / NT Kernel Logger）需要管理员权限启动跟踪会话。[已验证-来源：ETW 会话控制要求管理员的通用规则，learn.microsoft.com ETW 文档；逐字复核【待验证】]
- WMI `Win32_Process` 可无提权列出**当前用户**的进程 → 可做"进程快照"。
- 回溯性归因（"上周这个文件是哪个进程建的"）没有任何 Windows API 支持——文件系统不记录创建者进程。[已验证-来源：Windows 文件元数据模型（无 creator-process 字段）]
- **产品结论（诚实工程）**：进程归属只能做成 `inferred` 级推断——"事件时间窗内哪些 AI 工具进程在运行"（进程名快照 + 时间相关性），UI 明示置信度，绝不冒充精确归因。此结论已写入 PRD/INNOVATION_IDEAS（F11 的诚实边界）。

## Q5 能否判断文件属于哪个 AI Agent？

**不能由 OS 直接判断，但可以通过"信号 + 规则"高置信推断。**这正是 TraceNest 的核心差异化（见 INNOVATION_IDEAS F1/F2/F3）：

- 各 AI 工具留下稳定痕迹：官方数据目录（`~/.claude`、`~/.codex`…[见 AI_TOOLS_DIRECTORIES](AI_TOOLS_DIRECTORIES.md)）+ 项目根信号文件（`CLAUDE.md`/`AGENTS.md`/`.cursorrules`/`GEMINI.md`）+ 会话目录名编码项目路径（Claude Code `~/.claude/projects/<编码路径>`，官方文档确认）。
- 结论：`verified`（命中官方目录/信号文件）→ `likely`（目录模式）→ `inferred`（时间相关性）三级置信度体系在技术上完全可行。

## Q6 各平台实时监控的具体限制？

| 平台 | 机制 | 关键限制 | 状态 |
|---|---|---|---|
| Windows | ReadDirectoryChangesW（notify 封装） | 递归单句柄；缓冲溢出丢事件（需 overflow 计数）；网络盘不可靠 | [已验证-来源：notify windows.rs + diskplot watch:lost] |
| macOS | FSEvents（notify 默认后端） | 事件合并/延迟；per-file 粒度需 `kFSEventStreamCreateFlagFileEvents`；Desktop/Documents/Downloads 与外置卷触发 TCC 授权弹窗 | [官方文档]（Apple FSEvents/TCC 文档；细节待复核） |
| Linux | inotify | **无递归 watch**（每目录一个）；`fs.inotify.max_user_watches` 默认值随发行版（典型 8192–524288）会耗尽；fanotify 递归需 `CAP_SYS_ADMIN`（不用） | [已验证-来源：man7.org inotify(7)/fanotify(7)] |

**跨盘/挂载**：Windows 卷枚举（`GetLogicalDrives`）与 Linux `/proc/mounts` 枚举均无需特权；扫描需防挂载点循环（SquirrelDisk watch.rs 的平台差异策略可借鉴——Win/macOS 根递归、Linux 按需 focus）。[已验证-来源：竞品源码 + OS 惯例]

## Q7 Tauri 2 方案是否成立？

**成立。**[本机已验证]：本机 WebView2 Runtime 156.0.4314.58 已装（注册表确认）、Node 24 就绪；缺 Rust 工具链与 MSVC Build Tools（见 ENVIRONMENT_CHECK，官方链接已附）。

- WebView2 在 Win10 21H2+/Win11 系统自带，缺失时 Tauri 提供引导下载。[官方文档：tauri.app 前置条件]
- 无管理员要求；进程内 Rust command 架构适合扫描器/DB 读写与 UI 分离。[官方文档]
- 前端大数据渲染：treemap 需"可见节点上限 + 下钻"策略（diskplot 的"像素预算而非文件数预算"原则），canvas 兜底。[已验证-来源：diskplot 源码注释 + 通用可视化实践]

## Q8 SQLite 承载百万级行是否可靠？

**可靠。**

- WAL 模式支持"扫描批量写 + UI 只读查询"并发；单库理论/实践规模远超需求。[官方文档：sqlite.org/wal.html]
- 批量事务 + prepared statements 的插入吞吐对百万行级扫描足够（diskplot 用二进制快照达成 10s/200 万文件，SQLite 落库按批次 5k–10k 行设计，预计扫描+入库 100 万文件 ≤5 分钟，列为 M1 验收基准）。[设计目标，M1 实测]
- FTS5 文件名搜索原生支持。[官方文档：sqlite.org/fts5.html]
- DB 体积估算：~200B/行含索引 → 100 万文件约 0.5–2GB；提供旧扫描代清理。[估算，V0.1 实测修正]

## Q9 硬链接/junction 能否识别（去重计量）？

**能。**

- reparse point 检测：`FILE_ATTRIBUTE_REPARSE_POINT` 属性位 + `FSCTL_GET_REPARSE_POINT` 判类型（junction/symlink）；目录遍历中即可获得，无需提权。[官方文档：learn.microsoft.com]
- 硬链接：`GetFileInformationByHandle` 的 `nNumberOfLinks` / find data 字段。[官方文档]
- duvis 证实 Windows 下的硬链接/真实占用处理是普遍短板（其 Windows 版回退 apparent size）——TraceNest 做好此项即超大部分开源对手。[已验证-来源：duvis README]
- pnpm store/junction 不跟随、记录目标的策略已定（SYSTEM_DESIGN）。[已验证-来源：diskplot README 明示 junction 不跟随策略]

## Q10 无管理员权限的能力边界总表

| 能力 | Windows | macOS | Linux |
|---|---|---|---|
| 目录元数据扫描 | ✅（含批量枚举优化） | ✅（部分目录需 TCC 授权） | ✅（受目录权限限制） |
| 实时监控 | ✅ | ✅（TCC 例外） | ⚠️ watch 数量上限 |
| 单文件 USN 辅助建链 | ✅ | N/A | N/A |
| 全盘 MFT/USN 枚举 | ❌ 需管理员，不做 | N/A | N/A |
| 进程列表（当前用户） | ✅ WMI/Toolhelp | ✅ | ✅ /proc |
| 文件创建进程归因 | ❌ ETW 需管理员，不做 | ❌ EndpointSecurity 需特权，不做 | ❌ fanotify 需 CAP_SYS_ADMIN，不做 |
| 修改系统（注册表/环境变量） | ❌ 不做（产品约束） | ❌ | ❌ |

---

## 风险登记表（Top 5）

| # | 风险 | 影响 | 缓解 |
|---|---|---|---|
| 1 | `GetFileInformationByHandleEx` 路径在 Rust 侧的实现复杂度高于 std::fs | M1 基准不达标 | V0.1 先用 std::fs 保正确性，优化路径作为独立里程碑（架构已解耦） |
| 2 | 事件缓冲溢出导致监控"看起来漏事件" | 用户信任 | overflow 计数上 UI（诚实呈现），扫描兜底对账 |
| 3 | `OpenFileById` 父链解析的权限细节未验证 | F11 历史重建降级 | M2 实测；失败则父链仅用路径推导（当前 V0.1 方案，本就不依赖 USN） |
| 4 | Claude Code 新版本不再写 `~/.claude.json` 等结构漂移 | 规则库失准 | 规则包版本化 + 本机验证 + 社区适配器机制 |
| 5 | macOS/Linux 的 TCC/watch 上限使跨平台体验不一致 | V1.0 排期 | Windows 优先策略不变；平台差异已写入 PLATFORM_COMPATIBILITY |

## 对 V0.1 范围的最终建议

**全绿灯**：回顾式扫描（免管理员）、实时监控（notify）、SQLite 索引、信号文件归属、AI 工具知识库、置信度体系、去重计量基础（reparse 检测）——V0.1 范围（PRD §3）技术可行性**全部确认，无需缩减**。
**后置且诚实标注**：进程级归因（inferred 替代）、USN 父链（可选增强，M2 验证）、历史重建（V0.3，基于快照差分而非 USN 回放）。
