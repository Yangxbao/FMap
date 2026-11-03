# TraceNest 系统设计（SYSTEM_DESIGN）

> 版本：v0.9（夜间研究稿） · 日期：2026-11-03
> 依赖阅读：[TECH_FEASIBILITY.md](../research/TECH_FEASIBILITY.md)（能力边界）、[DATABASE.md](DATABASE.md)（存储）、[SECURITY.md](SECURITY.md)（安全）

---

## 1. 总体架构

```
┌─────────────────────────────────────────────────────────────┐
│  Tauri 2 应用进程（普通用户权限，启动时自我断言非提权）        │
│                                                             │
│  ┌─────────────── WebView 前端 ───────────────┐              │
│  │  React 18 + TypeScript + Tailwind CSS      │              │
│  │  页面：总览/地图/浏览器/识别中心/项目资产/   │  invoke()    │
│  │        时间线/监控/详情/报告/设置            │◄────────────►│
│  │  可视化：squarified treemap（自研，必要时    │              │
│  │          降级 D3），虚拟滚动表格             │              │
│  └────────────────────────────────────────────┘              │
│                     ▲ Tauri IPC Commands（见 §3）            │
│  ┌─────────────── Rust 核心 ──────────────────┐              │
│  │  scanner    多线程目录扫描引擎（可中断）     │              │
│  │  watcher    notify 实时监控 + 事件去抖       │              │
│  │  rules      AI 工具目录知识库 + 信号文件规则 │              │
│  │  attributor 归属推断 + 置信度评分            │              │
│  │  store      SQLite (rusqlite, WAL) 读写层   │              │
│  │  engine     任务调度（扫描/监控任务队列）    │              │
│  └────────────────────────────────────────────┘              │
└─────────────────────────────────────────────────────────────┘
   本地文件系统（只读访问）           本地 SQLite 数据库
   C:\ D:\ E:\ ...                   %APPDATA%/TraceNest/tracenest.db
```

**选型理由**（与用户建议的技术栈一致）：
- **Tauri 2**：Windows 上用系统 WebView2，体积小；Rust 后端天然适合高吞吐文件遍历；无需管理员权限。前端 React 复用生态。
- **notify**：跨平台 watcher（Windows=ReadDirectoryChangesW / macOS=FSEvents / Linux=inotify），与自研 watcher 细节解耦。
- **SQLite (rusqlite + bundled)**：单文件、零运维、WAL 支持读写并发；FTS5 支持文件名搜索。
- **D3.js（或自研 treemap）**：squarified treemap 布局算法成熟；大性能风险场景有 canvas 兜底预案。

---

## 2. 核心模块设计

### 2.1 scanner（扫描引擎）

- **遍历**：`std::fs::read_dir` 逐目录迭代（Windows 底层为 `FindFirstFileEx` 系）。每磁盘/每根目录一个任务，`rayon` 或手写线程池并行；**跳过**符号链接、挂载点（防循环）、网络盘（默认跳过，可配置）、排除模式（默认排除 `Windows`、`Program Files` 可配置但保留系统目录建议排除）。
- **性能路径（竞品研究已验证）**：Windows 上可改用 `GetFileInformationByHandleEx(FileFullDirectoryInfo)` 批量列目录（一次调用返回整目录条目+大小，配合 `\\?\` 长路径），**无需管理员权限**即可达到秒级（diskplot 用此路径实现 214.9 万文件 10s）。V0.1 先以 std::fs 实现保证正确性，M1 基准不达标时切换该路径（见 DEVELOPMENT_PLAN 风险表）。内存布局可参考列式 typed-array 设计（约 60B/文件）再批量落库。
- **产出**：每文件一条记录 `(volume, path, size, mtime, ctime, attributes, reparse_target?, parent_id)` → 批量入 DB（事务内 prepared statement，批次 5,000–10,000 条）。
- **可中断/续传**：扫描任务表记录 `(root, status, cursor)`；中断后从 cursor 所在目录继续。目录 mtime 未变可整目录跳过（增量续扫的优化项，V0.1 可选）。
- **限速与体验**：进度事件通过 Tauri 事件推送到前端；扫描不阻塞 UI 查询（WAL 读写分离）。
- **只读保证**：仅调用只读元数据 API；不打开文件内容（V0.1）；所有句柄以 `FILE_READ_ATTRIBUTES` 级别语义访问（通过 std::fs 元数据接口实现）。

### 2.2 watcher（实时监控）

- 基于 `notify`（推荐 `notify-debouncer-full` 去抖）。监视列表默认 = 已识别的 AI 工具数据目录 + 用户选中的项目目录（避免全盘 watch 的句柄/性能风险）。
- 事件模型：`{ ts, kind(Create|Modify|Remove|Rename), path, size?, watch_root }` → 写入 `events` 表（见 DATABASE）→ 推送前端事件流。
- **诚实边界**：事件不携带进程归属（无管理员权限无法归因）；事件可能丢失（缓冲区溢出、子树过深），UI 提供 watcher 健康状态（overflow 计数）。

### 2.3 rules（知识库与规则引擎）

- 规则包 = 版本化 JSON，随应用打包，支持本地文件导入（不联网）。
- 两类规则：
  1. **目录规则**：`{tool, os, pathPattern (支持 %APPDATA% 等展开), kind, contentHints, riskLevel, docUrl, source}`，匹配 = 目录归属 `verified`；
  2. **信号文件规则**：`{tool, fileName (CLAUDE.md/AGENTS.md/.cursorrules...), location (project-root|anywhere), confidence}`，匹配 = 目录被工具工作过。
- 引擎在扫描管线内增量匹配（路径规则 O(前缀匹配)，信号文件按文件名集合查表）。

### 2.4 attributor（归属与置信度）

- 输入：文件/目录记录 + 规则匹配结果 + （V0.2+）项目指纹。
- 输出：`{tool_id?, project_id?, confidence: verified|likely|inferred, basis: [rule_id|signal|parent]}`。
- 传播规则：子节点继承父目录归属（目录级归属向下传播，置信度衰减不传递——继承即同置信，但 basis 记录 "inherited-from:<dir>"）。
- **绝不输出无依据结论**：无匹配 → `unattributed`，UI 显示中性色。

### 2.5 store（SQLite 层）

见 [DATABASE.md](DATABASE.md)。要点：WAL、批写、FTS5 文件名索引、扫描任务与规则版本入库（行为可审计）。

---

## 3. Tauri IPC 命令清单（草案）

| Command | 方向 | 说明 |
|---|---|---|
| `scan_start(roots, excludes)` | →core | 启动扫描任务（异步，进度走事件） |
| `scan_status()` / `scan_cancel()` | →core | 任务状态/取消 |
| `query_volumes()` | →core | 磁盘列表与容量 |
| `query_tree(volume, path, depth, page)` | →core | treemap/树数据（服务端聚合，只返回节点摘要） |
| `query_files(filter, sort, page)` | →core | 文件浏览器分页查询 |
| `query_file_detail(id)` | →core | 来源详情（含依据链） |
| `query_tools()` / `query_projects()` | →core | 识别中心/项目列表 |
| `query_search(q)` | →core | FTS5 文件名搜索 |
| `watch_get/set(toggle, roots)` | →core | 监控配置 |
| `events_recent(limit)` | →core | 事件流 |
| `settings_get/set` | →core | 设置（本地 JSON） |
| `report_export(type, path)` | →core | V0.2：导出报告（用户选路径，core 不擅自写用户区） |

事件推送（core→前端）：`scan://progress`、`watch://event`、`watch://overflow`。

---

## 4. 前端设计要点

- **页面结构**：左侧固定导航（10 个页面），顶栏（全局搜索、主题切换、扫描状态徽标），内容区。与 [prototype/index.html](../../prototype/index.html) 原型一致。
- **性能**：
  - treemap：服务端按 `query_tree` 分层返回（每层聚合节点 ≤ 5000），点击下钻再取子层；DOM 渲染上限 20,000 节点，超出转 canvas（V0.1 预留接口）。
  - 表格：虚拟滚动（react-window 或自研），分页由 DB 层支撑。
  - 所有长列表查询必须命中索引；`query_tree` 用预聚合表（见 DATABASE §3）。
- **主题**：CSS 变量双主题（深色默认，浅色可选），Windows Fluent 风格：圆角 4px、细边框、克制的动效（≤150ms，无夸张动画）。
- **诚实性 UI**：置信度三色（verified=实线绿 / likely=虚线黄 / inferred=灰点）；任何"AI 判定"都可展开依据链。

---

## 5. 关键技术决策记录（ADR 摘要）

| # | 决策 | 理由 | 放弃的备选 |
|---|---|---|---|
| ADR-1 | 不做全盘 MFT 扫描 | FSCTL_ENUM_USN_DATA 需管理员权限 | WizTree 路线（需提权，违反约束） |
| ADR-2 | 目录级归属优先，文件级指纹延后 | 只读元数据即可完成 V0.1 价值闭环 | 立即做内容哈希（隐私成本前置） |
| ADR-3 | SQLite 单文件 + WAL | 零部署成本，读写并发够用 | LMDB/DuckDB（生态/查询形态不合） |
| ADR-4 | 自研 treemap 布局优先 | 布局算法约 200 行，避免整包引入 D3；D3 保留为备选 | 直接全量引入 D3（体积） |
| ADR-5 | 规则包本地导入而非在线更新 | 隐私原则（零出站） | 自动在线更新规则（需用户显式开启才可考虑） |
| ADR-6 | 事件不声称进程归属 | 无管理员权限无法验证，夸大即失信 | ETW 归因（需提权） |

---

## 6. 目录结构（规划，V0.1 开工时创建）

```
TraceNest/
├─ src/                    # React + TS 前端
│  ├─ pages/               # 10 个页面
│  ├─ components/          # 通用组件（VTable, Treemap, ConfidenceBadge...）
│  └─ lib/ipc.ts           # invoke 封装
├─ src-tauri/
│  ├─ src/
│  │  ├─ commands/         # Tauri command 层
│  │  ├─ scanner/ watcher/ rules/ attributor/ store/ engine/
│  │  └─ main.rs
│  ├─ tauri.conf.json      # CSP、权限最小化配置
│  └─ Cargo.toml
└─ rules/                  # 内置规则包（版本化 JSON）
```
