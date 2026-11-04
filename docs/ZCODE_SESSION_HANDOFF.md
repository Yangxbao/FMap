# FMap 会话交接文档（ZCODE_SESSION_HANDOFF）

> **更新时间**：2026-11-05 · **来源会话**：完成 M0/M1/M2 全部开发与验收的会话
> **用途**：新 ZCode 会话无缝接手——只上传/阅读本文件即可完整理解项目、继续开发
> **本文件随每次 ZCode 会话结束而更新（覆盖）**，历史版本在 Git 历史中

---

## 1. 项目基本信息

| 项 | 值 |
|---|---|
| **项目名称** | FMap（File Map，2026-11-04 由 TraceNest 更名） |
| **中文定位** | AI 文件资产地图 |
| **产品定位** | 本地运行、跨平台（Windows 首发）、**只读**的文件资产追踪工具——帮助用户识别电脑中由 AI 工具产生的文件，理解文件来源与用途 |
| **产品口号** | 让 AI 在电脑上留下的每一个文件，都有迹可循 |
| **工作区路径** | `D:\.zcode\workspace\draft-b6f1ac71-2534-44de-9e25-045ca2d12394\TraceNest` |
| **Git** | 分支 main · 无远程仓库（用户未授权推送）· Git 身份：赵子龙 <3296648312@qq.com> |
| **技术栈** | Tauri 2 · Rust（workspace：core/database/platform 三 crate）· React 19 + TypeScript 5.9 · Tailwind CSS v4 · SQLite（rusqlite bundled, WAL, FTS5）· vitest |
| **工具链** | Rust 1.99.1 stable-msvc → `D:\rust`（RUSTUP_HOME/CARGO_HOME 已 setx 持久化，cargo\bin 已入用户 PATH）· MSVC v143 + Windows SDK 26100 → `D:\VS2022BuildTools` · Node v24.21.0 · WebView2 156.x |
| **数据库** | `%APPDATA%\io.github.tracenest.desktop\tracenest.db`（SQLite v4，WAL）· **identifier 严禁更改**（兼容历史数据） |
| **数据规模** | paths 508,032 条（真实扫描）· attributions 4,245 条 · tools 15 · rules 29(enabled) · 扫描代 4 |

**工具链安装位置（D 盘重定向，勿装到 C 盘）**：
- `D:\rust`（RUSTUP_HOME=`D:\rust\rustup` · CARGO_HOME=`D:\rust\cargo`）
- `D:\VS2022BuildTools`（MSVC v143 + Windows SDK；VS Installer 共享组件仍在 C 盘，不可避免）
- C 盘仅剩约 60 GB——新工具链/依赖**禁止装 C 盘**

## 2. 项目历史和当前进度

### 已完成里程碑

| 阶段 | 内容 | 提交 |
|---|---|---|
| **M0** | 应用骨架：workspace 三 crate + Tauri 壳 + React 前端 + SQLite 层 + 只读门禁 | 5c1a304, 7295afb |
| **M1 代码层** | 扫描引擎（七态状态机/取消/排除/链接不跟随）+ 批量 DAO + 识别中心 IPC | 8a5d4b1 |
| **M1 真机验收** | 工具链安装 + 17/17 单测 + clippy 0 + E 盘 27.9 万文件 14.7s + 桌面端 32.6 万条入库 | 9b685ae, f211601 |
| **品牌更名** | TraceNest → FMap（仅品牌，零功能变更；内部标识保留） | 169c6ff |
| **M2 主体** | 规则库（15 工具/29 规则）+ 归属引擎（四级证据）+ 识别中心 + IPC | 75e2dde |
| **M1 收尾** | 批量事务(110K 行/秒) + 异常恢复(instance_id) + 能力标签 + 取消一致性 | fd4c593 |
| **M2 验收收尾** | 归属语义修正（v4 source_behavior）+ 迁移安全重构 + 实机验收截图 | fd4c593(另一部分), 169c6ff |
| **文档清理** | 13 份过程文档归档于 Git 历史，保留核心 18 份 | aac9e6f |
| **审查修正** | 三层分模型 + File ID 深评 + USN 权限纠正 + M3-A 授权默认关闭 | 1d29bbb |
| **最终修正** | 证据能力边界表 + M3-C 语义重写 + 文件身份精确化 + 估算标记 | 45ca8da |

### 当前状态（以 45ca8da 为准）

- **实际具备**：只读多磁盘扫描（批量事务 110K 行/秒）· 异常退出恢复 · 取消一致性 · 规则库识别（15 工具）· 信号文件检测 · 归属引擎（四级证据/冲突/继承）· 识别中心真实数据页面 · 只读门禁
- **实机验证过**：多磁盘扫描 + 入库（508,032 条）· 识别中心真实渲染（截图存档 docs/evidence/）· v2→v3→v4 迁移 · 孤儿恢复（三次）· UI 取消一致性（数字精确对账）
- **代码完成但未实机验证**：无——M2 全部功能已实机跑通（UI 详情页交互为最近一次截图确认）
- **尚未实现**：M3 全部（文件身份/事件/来源证据/候选/确认）· USN 事件流 · 进程关联 · 浏览器记录 · 实时监控（notify）· 磁盘地图/文件浏览页的真实数据接入 · NSIS 打包

## 3. 当前代码结构和关键实现

### Rust workspace（4 crate）

```
Cargo.toml                    # workspace 根（version 0.1.0, edition 2021, license MIT OR Apache-2.0）
crates/core/                  # 与 OS 无关的共享类型与纯逻辑
  src/lib.rs                  #   APP_NAME/APP_VERSION · ScanStatus 七态 · CoreError · format_bytes
  src/scanner.rs              #   扫描引擎核心：scan_root() 只读遍历 · WalkEntry/WalkBatch · 取消 · 排除目录 · 链接不跟随
  src/attribution.rs          #   归属引擎纯逻辑：evaluate_node() 后缀窗口匹配 · 四级证据 · 互证升级 · 冲突检测
crates/database/              # SQLite 层（rusqlite bundled + WAL）
  src/lib.rs                  #   Database::open/migrate(v1→v4 阶段事务+幂等补列)/recover_orphaned_scans
  src/scan_writer.rs          #   run_scan_to_db()：遍历→批量事务写(5000/批)→聚合→终态（一致性核心）
  src/rules_loader.rs         #   内置规则包 JSON→tools/rule_packs/rules 表（幂等）
  src/attribution_pass.rs     #   归属 pass + 识别中心查询 API（tools_overview/tool_top_paths/build_full_path 等）
crates/platform/              # 平台抽象
  src/lib.rs                  #   FileSystemProvider trait · Windows 磁盘发现(Win32 API) · 提权检测 · PlatformCapabilities
apps/desktop/src-tauri/       # Tauri 壳
  src/lib.rs                  #   AppState(db+scans+instance_id) · setup(提权自检+规则加载+孤儿恢复) · invoke_handler
  src/commands.rs             #   M0/M1 命令：get_system_status/list_volumes/get_db_stats/scan_start/status/cancel
  src/commands_m2.rs          #   M2 命令：run_attribution/get_tools_overview/get_tool_detail/set_rule_enabled
  tauri.conf.json             #   productName FMap · identifier io.github.tracenest.desktop（严禁改）· CSP · 窗口
apps/desktop/ui/              # React 前端（Tailwind v4）
  src/lib/ipc.ts              #   invoke 封装（浏览器模式降级）+ 全部类型定义
  src/lib/format.ts           #   formatBytes + PAGE_IDS 路由
  src/components/Layout.tsx   #   侧栏 9 页导航（M 徽标）+ 顶栏后端状态
  src/pages/Overview.tsx      #   总览：真实系统状态 + 磁盘列表 + ScanCard（开始/取消/轮询）
  src/pages/Tools.tsx         #   识别中心：真实归属数据 + 规则启停 + 两维度说明框
  src/pages/Settings.tsx      #   设置：只读展示默认策略
  src/pages/Placeholders.tsx  #   磁盘地图/文件浏览等 DevPlaceholder 占位
  src/__tests__/              #   vitest：format 7 + ipc 3
```

### 关键机制（新会话必须理解的实现）

**扫描引擎**（scanner.rs）：`scan_root(root, options, cancel, batch_size, on_batch)` 只读遍历；每目录检查取消标志；无权目录计入 denied_count 继续；排除 `$RECYCLE.BIN` 等；junction/symlink 记录但不深入；产出 `WalkEntry`（rel_path/name/is_dir/size/mtime/depth/attrs）分批交给 `on_batch`。

**批量事务写库**（scan_writer.rs）：`run_scan_to_db(db, scan_db_id, volume_id, root, options, cancel, batch_size, on_progress)` = 遍历 + 每批 5000 条事务提交（失败回滚该批+前批完好+终态 failed）+ 聚合 + finish_scan(终态+end_note)。取消时缓冲冲刷（不丢已发现记录）→ 终态 cancelled + "非完整快照"标注。

**异常退出恢复**（lib.rs）：每 Database 实例生成 instance_id；`recover_orphaned_scans()` 把 `status='running' AND instance_id<>当前` 的扫描标 failed + end_note；应用 setup 时调用。

**规则库**（rules_loader.rs + rules/builtin-rules-v1.json）：JSON 数据文件（15 工具/29 规则）→ tools/rule_packs/rules 表；同版本幂等跳过；`set_rule_enabled()` 启停；引擎只读 `enabled=1` 的规则。

**归属引擎**（attribution.rs + attribution_pass.rs）：
- 纯逻辑 `evaluate_node(chain, is_dir, rules)` — 祖先名称链的**后缀窗口匹配** + **祖先链窗口命中（互证升级）**；dir 规则只作用目录；权重→级别（≥1.0=Verified / ≥0.7=Likely / 否则 Possible）
- pass：加载 enabled 规则 → 加载 paths 树（内存 HashMap）→ 逐节点评估 → **Verified 目录归属向下继承**（仅目录规则来源；信号 Likely 不继承）→ 冲突检测（≥2 工具同级别）→ 清本扫描代旧结论 → 批量事务写 attributions
- 查询 API：`tools_overview(scan_db_id)` / `tool_top_paths` / `build_full_path(id)`（迭代父链）/ `tool_rules(tool_key)` / `tool_conflicts`

**迁移机制**（lib.rs migrate()）：**阶段事务**（版本号与 DDL 同事务）+ **逐列幂等**（pragma_table_info 检查后 ALTER）+ 崩溃恢复安全（半迁移自动续跑）。当前 v4：scans.instance_id/end_note、rules.enabled/weight/evidence、tools.kind、attributions.source_behavior。

**前端 IPC**（ipc.ts）：`backendAvailable` 检测（无 Tauri 运行时→IpcError 降级）；全部命令在 `api` 对象；组件不直接调 invoke。

### SQLite Schema v4（全表）

| 表 | 用途 | 关键列 |
|---|---|---|
| meta | schema_version 等 | key,value |
| volumes | 磁盘登记 | root(UNIQUE), label, total_bytes, free_bytes, fs_type, is_removable |
| scans | 扫描代（七态） | started_at, status(running/done/cancelled/failed/partial/access_denied/volume_offline), roots, files_scanned, dirs_scanned, errors_count, app_version, instance_id, end_note |
| paths | 路径节点（50 万+） | volume_id, parent_id, name, depth, is_dir, size, mtime, ctime, attrs, scan_id, deleted_at |
| paths_fts | 文件名全文搜索 | name |
| tools | AI 工具 | key(UNIQUE), name, vendor, kind, color, builtin |
| rule_packs | 规则包 | pack_name, version, rule_count, source |
| rules | 规则 | tool_id, kind(dir/signal), os, pattern, location, confidence, weight, kind_tag, risk_level, doc_url, source, evidence, enabled |
| attributions | 归属结论（4,245） | path_id, scan_id, tool_id, confidence(verified/likely/possible), score, basis(JSON), source_behavior(unknown) |
| agg_tool_size | 目录×工具聚合 | path_id, scan_id, tool_id, direct_bytes, total_bytes |
| events | 实时事件（M5 用） | ts, kind, volume_id, path, size, watch_root |
| watch_roots | 监控根（M5 用） | path, enabled |

### 文件身份与来源行为设计（M3-A 目标，**未实现**）

三层分模型 + 六职责表（设计见 M2_FINAL_HANDOFF.md §17）：
- `file_entities`（卷序列号+FileId 主键+校验位+降级链字段）→ 身份解析
- `file_events`（append-only 原始事件，含 evidence 引用）→ 不可被自动判断覆盖
- `source_evidence`（独立证据表）→ 自动候选只引用证据 id
- `source_candidates`（自动候选，规则更新可整体重建）→ 不触碰确认
- `source_confirmations`（用户确认，append-only 保留修正历史）→ 最高可信
- `attributions`（M2 工具关联）→ **原样保留**，不转换成来源行为

## 4. 已确认的技术决策（不得擅自推翻）

| # | 决策 | 理由 |
|---|---|---|
| 1 | **工具关联 ≠ 文件来源**：.codex 内的文件是"工具管理的数据"，不等于"AI 生成的代码" | 位置≠来源；CLAUDE.md 只证明工具工作过 |
| 2 | **来源行为三级分模型**：原始事件与证据 → 自动来源判断（候选）→ 用户确认；三层分开存储 | 用户确认最高可信，候选可重算，证据不可覆盖 |
| 3 | **来源行为自动分析恒填 unknown**：M2/M3-C 归属结论不直接产出 AI_CREATED/AI_MODIFIED | 只有官方工具事件/进程可靠归因/用户确认才可产候选（M3-C） |
| 4 | **UNKNOWN 是合法且永久保留的状态** | 宁可显示未知，不可编造来源 |
| 5 | **时间共现只能证明时间关联** | 时间窗 ≠ 来源行为 |
| 6 | **迁移必须阶段事务 + 逐列幂等** | 版本号与 DDL 同事务；崩溃可续跑（v2→v3 事故教训） |
| 7 | **文件身份 = 卷序列号 + FileId + 降级链，非纯路径** | File ID 不保证长期唯一（MFT 重用）；跨卷移动=新身份 |
| 8 | **File ID 不是永久唯一标识** | MFT 记录号删除后可能重用（微软文档警告） |
| 9 | **扫描只写 paths/scans/attributions；不触碰用户文件** | 只读承诺（CI 门禁 tests/check-readonly.mjs） |
| 10 | **规则数据与引擎分离**（JSON 数据文件） | 社区适配器扩展入口 |
| 11 | **身份键不含盘符**（用卷序列号/卷 GUID） | 重挂载盘符变化不影响 |
| 12 | ** Verified 目录继承仅限目录规则来源** | 信号 Likely 不向下传播（避免过度归属） |
| 13 | **单实例扫描假设**（双实例并发边界已文档化） | 彻底方案需进程存活检测（M3+） |

## 5. 用户明确要求的安全红线（完整记录，违反即停下）

1. **不删除、移动、重命名或修改用户文件**——只读承诺；CI 门禁 `tests/check-readonly.mjs`
2. **不读取文件正文作为默认功能**（默认功能只用元数据）
3. **不默认计算 Hash**
4. **不读取聊天记录、AI 会话数据库和 JSONL**
5. **不擅自提权或绕过访问控制**
6. **不关闭 Defender、Smart App Control 等安全机制**（本机 SAC = 强制模式 0x1）
7. **不添加 Defender 排除项**
8. **不自动安装工具链或修改系统配置**（工具链已装于 D 盘；如需更多组件先问用户）
9. **不未经授权推送 GitHub**（无远程仓库；用户未授权建仓/推送）
10. **不使用子代理**——用户体验套餐限额，子代理零产出且浪费上下文；**一切工作主会话直接做**
11. **不擅自迁移项目目录**（工作区路径保持）
12. **隐私敏感功能默认关闭，必须由用户主动授权**（逐项授权，不用笼统"允许追踪"）
13. **文件身份采集默认关闭**（M3-A 落地时执行此决策）
14. **不进行未经授权的后台监控**
15. **不得编造扫描结果、测试结果和文件来源**（宁可显示未知）
16. **不启动 M3 正式开发**（除非用户明确安排）

## 6. 当前遗留问题

| # | 问题 | 严重程度 | 状态 |
|---|---|---|---|
| 1 | 双实例并发扫描：instance_id 方案假设单实例，进程存活检测未做 | 低 | 已文档化边界，M3+ 处理 |
| 2 | 总览页"扫描记录"统计不自动刷新（快照式） | 低 | M3-A 顺带解决 |
| 3 | 应用正常关闭时无主动取消在跑任务的钩子（靠启动恢复兜底） | 低 | M3-A 顺带解决 |
| 4 | NSIS 打包产物名（FMap）未验证 | 低 | 打包时验证 |
| 5 | 跨平台（macOS/Linux）未测试 | 中 | M1 后期/M3-E |
| 6 | ZCode 规则置信=likely（无官方文档，本机观察）；豆包 Roaming 侧未命中 | 低 | 待全盘扫描复核 |
| 7 | 平台能力标签 `can_scan_metadata` 在 Windows 实现为 true 但 UI 能力卡曾显示 ⏳（旧截图遗留，新构建已正确） | 已修 | 新构建已验证 |
| 8 | `cargo test --workspace` 偶被 SAC 拦截新二进制（os error 4551）→ 逐 crate 运行等效 | 环境问题 | 已文档化绕过方式；SAC=0x1 强制模式 |
| 9 | scan_writer `flush()` 内 `return` 语义依赖回调闭包嵌套层级（脆弱但正确，有注释） | 低 | 已注释说明 |
| 10 | FileIdExtdDirectoryInfo 枚举类的可用 Windows 版本范围未实测 | 待验证 | M3-A 时验证 |
| 11 | attributions 的 source_candidates 表和 source_confirmations 表未实现（M3-A 设计目标） | 设计目标 | M3-A 落地 |
| 12 | 文件身份采集功能未实现（M3-A 设计目标，默认关闭） | 设计目标 | M3-A 落地 |

## 7. 后续开发路线（M3-A → M3-E，**均已规划未开发**）

| 阶段 | 目标 | 依赖 | 状态 |
|---|---|---|---|
| **M3-A** | 文件资产基础模型：file_entities(卷+FileId) + file_events + source_evidence + source_candidates + source_confirmations 五表；用户确认 UI；M2 归属兼容 | v5 迁移方案需用户确认 | **规划完成，未开发** |
| **M3-B** | 基础事件追踪：扫描差分 → FILE_DISCOVERED/MODIFIED；可选 RDCW 目录监视 | M3-A | 规划完成 |
| **M3-C** | AI 工具行为关联：工具关联+时间关联（**不产 AI_CREATED/MODIFIED**）| M3-A/B | 规划完成（语义已修正） |
| **M3-D** | 普通文件来源：浏览器下载记录集成（可选授权）+ 用户标记 UI | M3-A | 规划完成 |
| **M3-E** | 高级持续追踪：USN Journal（需管理员评估）+ 后台监视 + 跨平台 | M3-B + 用户逐项授权 | 规划完成 |

**下一步**：M3-A（文件资产基础模型）。开工前**必须**先向用户确认两件事：
1. v5 迁移方案（新增 file_entities/file_events/source_evidence/source_candidates/source_confirmations 五表 + attributions.entity 关联，全部非破坏 ALTER）
2. 文件身份采集授权项设计（**默认关闭**，用户授权后开始，撤销可删数据——详见 M2_FINAL_HANDOFF.md §17）

**不能重复开发**：M0/M1/M2 全部功能（扫描/规则/归属/识别中心/IPC）——已实现且测试通过。
**只是设计方案**：六职责模型/事件模型/来源行为候选——文档中有，代码里没有。
**必须先征求用户同意**：全盘扫描、安装任何组件、Git 推送、identifier 更改。

## 8. 本次任务完整记录

### 任务目标
用户安排：M2 最终验收收尾 + 全面文件来源追踪架构规划 + 单文件交接（M2_FINAL_HANDOFF.md）+ 会话交接文档（本文件）。

### 实际执行（按序）

**第一轮：M2 主体开发**（提交 75e2dde）
- 工具链安装：Rust 1.99.1 → D:\rust（minimal profile，跳过 rust-docs）· MSVC v143 → D:\VS2022BuildTools（用户授权后安装）
- 编译修复 3 轮：windows 0.61 切片式 API / core 缺 dev-deps / tauri Manager 导入 + 尾表达式借用 ×2
- E 盘真实扫描：27.9 万文件 14.7s（examples/scan.exe）
- 桌面应用端到端：启动→识别中心→E 盘扫描→实时进度→326,521 条入库（node:sqlite 验证）

**第二轮：M2 交接报告审查修正**（提交 1d29bbb）
- 用户要求：修正来源行为三层分模型设计 + File ID 深评 + USN 权限结论纠正 + M3-A 隐私具体化 + 证据补档
- 三张实机截图存档 docs/evidence/（总览/识别中心/Codex 详情）
- 发现并记录：并发 BUSY 缺陷（busy_timeout 30s + 终态重试修复）

**第三轮：文档清理**（提交 aac9e6f）
- 13 份过程文档删除（Git 历史保留）· README/ROADMAP/DEVELOPMENT_PLAN 引用修复
- .tmp-server 临时目录删除

**第四轮：M2 最终验收收尾 + M3 规划**（提交 45ca8da + fd4c593）
- v4 迁移（source_behavior）+ 三层分模型落地
- 迁移安全重构（阶段事务+幂等补列+崩溃恢复测试）
- 证据能力边界表 + M3-C 语义重写 + 六职责模型设计
- 识别中心 UI 实机验收（截图确认真实数据渲染）
- M3-A~E 路线图

**第五轮：文档清理 + 本文件**（提交 aac9e6f + 45ca8da + 本提交）
- 13 份过程文档删除（Git 历史保留）· README/ROADMAP/DEVELOPMENT_PLAN 引用修复
- .tmp-server 临时目录删除
- 审查修正报告

### 实际执行的命令与结果

| 命令 | 结果 |
|---|---|
| `cargo test --workspace` | ✅ **40/40**（core 18 + database 19 + platform 3）|
| `cargo clippy --workspace` | ✅ 0 警告 0 错误 |
| `cargo fmt --all && cargo fmt --check` | ✅ |
| `npm run typecheck` / `build` / `test` | ✅ / ✅ / **7/7** |
| `node tests/validate-schema.mjs` | ✅ schema v4 结构验证 |
| `node tests/check-readonly.mjs` | ✅ 只读门禁 |
| `node tests/make-fixture.mjs 200 5` | ✅ 400 目录/1000 文件确定性树 |
| `node .tmp-review-patch.mjs` 系列 | ✅ 文档更新脚本 |
| `cargo run -p tracenest-database --example attribution_real` | ✅ 真实库归属分析 930ms |
| `cargo run -p tracenest-core --example scan -- <目录>` | ✅ 七目录真实扫描（含 .zcode 54,486 文件） |
| CUA 截图验证 | ✅ 总览 v4 / 识别中心 / Codex 详情 / 设置页 |

### Git 提交链（完整）

```
45ca8da docs(m2-final): 审查修正——来源行为三层分模型/证据能力边界表/M3-C语义重写/文件身份精确化/M3-A授权默认关闭
1d29bbb docs(m2-final): 审查修正轮——来源行为三层分模型/文件身份深评/USN权限纠正/M3-A隐私具体化/证据补档
aac9e6f chore(docs): 清理过程文档——13 份已归档于 Git 历史，保留单文件交接制入口
fd4c593 fix(m2-final): 归属语义修正(工具关联≠来源行为,v4)+迁移崩溃安全重构+识别中心实机验收+长期方向与M3规划
75e2dde feat(m2): AI 工具规则库+归属引擎+识别中心——15工具/29规则, 真实库4245条归属结论(930ms)
f211601 fix(m1): 四项遗留清零——批量事务(110K行/秒)/异常恢复(instance_id)/能力标签/取消一致性
169c6ff brand: 全项目更名 TraceNest → FMap（仅品牌，零功能变更）
9b685ae feat(m1): 工具链就绪+真机验收——17/17 单测全绿、clippy 清零、E盘32.6万条真实路径入库
8a5d4b1 feat(m1): 扫描引擎代码层完成——scanner 七态状态机/取消/错误计数 + 批量DAO/size聚合 + IPC三命令 + 前端扫描卡
7295afb docs: M0 交付文档四件套 + 开源合规文件（CONTRIBUTING/SECURITY/README 更新）
5c1a304 chore: M0 初始提交——研究文档、架构设计、Tauri 2 桌面应用骨架与可运行前端
```

### 当前工作区状态
- `git status --short` → 0 行（干净）
- 未推送远程；无远程仓库

## 9. 给新 ZCode 会话的启动指令

以下内容直接复制到新 ZCode 会话即可：

---

**启动指令（复制以下全部内容）**：

你是 FMap 项目（File Map，AI 文件资产地图）的开发工程师，接手一个已完成 M0/M1/M2 的项目。

**第一步：阅读交接文档**
完整阅读 `docs/ZCODE_SESSION_HANDOFF.md`（本文件），理解项目全貌。

**第二步：核实项目状态**
1. `git branch --show-current && git log --oneline | head -6 && git status --short`（确认 main 分支、HEAD、干净工作区）
2. `cargo test --workspace` 或逐 crate 测试（确认测试仍然通过）
3. 只读查看数据库 `%APPDATA%/io.github.tracenest.desktop/tracenest.db`（schema v4、508,032 条 paths、4,245 条 attributions）

**第三步：阅读下一步相关材料**
- 下一阶段是 **M3-A（文件资产基础模型）**，规划见 `docs/M2_FINAL_HANDOFF.md` §17 与 §15/§16
- 相关源码：`crates/core/src/scanner.rs`（扫描引擎）、`crates/database/src/lib.rs`（迁移机制）、`apps/desktop/src-tauri/src/commands.rs`（IPC）
- 现有规则库：`rules/builtin-rules-v1.json`

**第四步：核实交接文档关键结论**
- 确认 `cargo test --workspace` 仍 40/40
- 确认 schema v4（不是 v3/v2）
- 确认 attributions 全部 source_behavior='unknown'
- 确认 tests/check-readonly.mjs 通过

**第五步：向用户汇报当前理解和下一步计划**
- 汇报你对项目架构、M3-A 目标和六职责模型的理解
- 提出 M3-A 的具体实施计划
- **等待用户安排后再执行开发任务**

**铁律（违反即停下询问用户）**：
1. 不使用子代理（体验套餐限额）
2. 不自动安装 Rust/MSVC/SDK 或其他系统工具（工具链已在 D:\rust 和 D:\VS2022BuildTools）
3. 不删除、移动、重命名或修改用户文件
4. 不读取文件正文、聊天记录、AI 会话数据库或 JSONL
5. 不默认计算 Hash
6. 不关闭 Defender、Smart App Control 或添加安全排除项
7. 不提权、不绕过访问控制
8. 不修改注册表或系统环境变量（RUSTUP_HOME/CARGO_HOME 已设好，不要动）
9. 不未经授权推送 GitHub 或创建仓库
10. 不擅自更改 identifier `io.github.tracenest.desktop`（兼容历史数据）
11. 不把计划中的功能写成已实现
12. 不为测试通过而伪造结果
13. 单文件交接制：阶段完成更新 `docs/ZCODE_SESSION_HANDOFF.md`
14. M2 已完成的扫描/规则/归属/识别中心功能**不要重复开发**
15. 来源行为（AI_CREATED 等）自动分析**只产候选且与用户确认分开存储**，且 M2 现状全部为 unknown

---

## 附：快速参考

| 场景 | 命令 |
|---|---|
| 跑 Rust 全测试 | `cargo test --workspace` |
| 跑单个 crate 测试 | `cargo test -p tracenest-database` |
| 跑前端测试 | `cd apps/desktop/ui && npm test` |
| 前端构建 | `cd apps/desktop/ui && npm run build` |
| schema 只读检查 | `node tests/validate-schema.mjs` |
| 只读门禁 | `node tests/check-readonly.mjs` |
| 合成文件树 | `node tests/make-fixture.mjs 200 5` |
| 真实库归属分析 | `cargo run -p tracenest-database --example attribution_real` |
| 真实目录扫描 | `cargo run -p tracenest-core --example scan -- <路径> [期望文件数] [期望目录数]` |
| 桌面应用开发 | `cd apps/desktop/ui && npm run dev` + `cd ../src-tauri && cargo tauri dev` |
| 生成图标 | `node scripts/make-icon.mjs` |

| 环境变量（已持久化） | 值 |
|---|---|
| RUSTUP_HOME | D:\rust\rustup |
| CARGO_HOME | D:\rust\cargo |
| PATH 包含 | D:\rust\cargo\bin |

| 安全门禁 | 位置 |
|---|---|
| 只读检查 | tests/check-readonly.mjs |
| Schema 验证 | tests/validate-schema.mjs |
| 前端测试 | apps/desktop/ui/src/__tests__/ |
| CI 门禁说明 | docs/development/ACCEPTANCE_CRITERIA.md |
