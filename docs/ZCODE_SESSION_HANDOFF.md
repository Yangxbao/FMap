# FMap 会话交接文档（ZCODE_SESSION_HANDOFF）

> **更新时间**：2026-11-05 · **来源会话**：完成 M3-A（文件资产基础模型）全部开发与验收的会话
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
| **工具链** | Rust 1.99.1 stable-msvc → `D:\rust`（RUSTUP_HOME/CARGO_HOME 已 setx 持久化）· MSVC v143 + Windows SDK 26100 → `D:\VS2022BuildTools` · Node v24.21.0 · WebView2 156.x · **本机 Windows 11 26200（25H2）** |
| **数据库** | `%APPDATA%\io.github.tracenest.desktop\tracenest.db`（SQLite，WAL）· **identifier 严禁更改**（兼容历史数据）· **真实库当前仍是 v4，将在应用下次启动时由已验收的迁移代码自动升 v5**（副本已验证无损，见 §8） |
| **数据规模** | paths 508,032 条（真实扫描）· attributions 4,245 条（source_behavior 全 unknown）· tools 15 · rules 29(enabled) · 扫描代 4 |

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
| **M2 验收收尾** | 归属语义修正（v4 source_behavior）+ 迁移安全重构 + 实机验收截图 | fd4c593, 169c6ff |
| **文档清理/审查修正** | 三层分模型 + File ID 深评 + 证据能力边界表 + 单文件交接制 | aac9e6f, 1d29bbb, 45ca8da, effa5c0 |
| **M3-A v5 迁移** | 五表（身份/事件/证据/候选/确认）+ volumes.volume_guid + attributions.entity_id；阶段事务+幂等；测试 22/22 | 40be846 |
| **M3-A 身份解析** | platform：卷 GUID + NTFS FileId 批量枚举（FileIdBothDirectoryInfo）+ 逐文件回退；真机验证 9/9 | （见提交链） |
| **M3-A 数据层** | source.rs（确认/撤销/证据/时间线/授权开关）+ identity_pass.rs（回填/重用嫌疑/关联）；测试 35/35 | （见提交链） |
| **M3-A IPC/前端** | 10 命令 + 退出取消钩子 + 授权卡/采集卡/来源确认面板；vitest 9/9 | e51a940 等 |
| **M3-A 验收** | clippy 0 + workspace 62/62 + schema 验证 v5 + 只读门禁 + **真实库副本无损迁移（508,032 paths 全保留）** | （本文件随附提交） |

### 当前状态（以最新提交为准）

- **实际具备**：M2 全部（只读多磁盘扫描/规则库识别/归属引擎/识别中心）**+ M3-A 全部**：
  - 文件身份模型：`file_entities`（卷 GUID + NTFS FileId 主身份，(size,mtime) 校验位，非 NTFS 弱身份降级）
  - 身份采集：**默认关闭**，设置页授权 → 总览页选卷回填（批量枚举与扫描同句柄；批量事务 5000/批；可取消）
  - 三层分模型落地：file_events（append-only）/ source_evidence / source_candidates（可整代替换）/ source_confirmations（append-only 修正历史）
  - 用户确认：AI 工具页 Top 路径「来源」面板——确认/修正/撤销（revoke=unknown），事件时间线 + 证据引用
  - MFT 重用检测：同 FileId 校验位变化 → `file_id_reuse_suspected` 待确认事件（不静默改写）
  - 撤销授权：仅停止（保留数据）或停止并删除身份域（paths/attributions 主体不动）
  - 遗留 #2/#3/#10/#11/#12 全部关闭（统计卡自动刷新 / 退出取消钩子 / FileId 枚举类选型 / 五表 / 身份采集）
- **实机验证过**：FileId 枚举与句柄互证（Win11 26200）· 真实库副本 v4→v5 无损迁移 · 全部 71 项自动化测试
- **尚未实现**：M3-B 全部（扫描差分 → FILE_DISCOVERED/MODIFIED、RDCW 监视、events 表填充）· M3-C/D/E · 进程关联 · 浏览器记录 · 磁盘地图/文件浏览页真实数据 · NSIS 打包

## 3. 当前代码结构和关键实现

### Rust workspace（4 crate）

```
Cargo.toml                    # workspace 根（edition 2021, MIT OR Apache-2.0）
crates/core/                  # 与 OS 无关的共享类型与纯逻辑
  src/lib.rs                  #   APP_NAME/APP_VERSION · ScanStatus 七态 · CoreError · format_bytes
  src/scanner.rs              #   扫描引擎：scan_root() 只读遍历 · WalkEntry · 取消 · 排除 · 链接不跟随
  src/attribution.rs          #   归属引擎纯逻辑：evaluate_node() · 四级证据 · 互证升级 · 冲突
crates/platform/              # 平台抽象
  src/lib.rs                  #   FileSystemProvider trait · Windows 磁盘发现 · 提权检测 · 能力声明
  src/identity.rs             #   M3-A 身份解析：volume_identity(卷GUID+序列号) ·
                              #   enumerate_dir_with_ids(FileIdBothDirectoryInfo 批量) · get_file_id 回退 ·
                              #   fs_supports_stable_file_id(仅 NTFS)
crates/database/              # SQLite 层（rusqlite bundled + WAL）
  src/lib.rs                  #   Database::open/migrate(v1→v5 阶段事务+幂等补列) · recover_orphaned_scans ·
                              #   get/set_meta · set_volume_guid
  src/scan_writer.rs          #   run_scan_to_db()：遍历→批量事务写(5000/批)→聚合→终态
  src/rules_loader.rs         #   内置规则包 JSON→tools/rule_packs/rules（幂等）
  src/attribution_pass.rs     #   归属 pass + 识别中心查询 API
  src/source.rs               #   M3-A 来源模型：behavior/event_kind/evidence_kind 常量 ·
                              #   identity_collection_enabled/set_enabled · revoke_identity_collection ·
                              #   confirm_source/revoke_source（证据→确认→事件三写）· path_source_detail ·
                              #   source_overview · identity_volumes
  src/identity_pass.rs        #   M3-A 身份回填：run_identity_backfill（授权门禁→卷GUID→NTFS主身份/
                              #   弱身份降级→最新扫描代 rel 映射→批量 upsert→MFT 重用嫌疑→归属集合式关联）
  examples/identity_real.rs   #   真实库迁移验收例子（对副本运行，输出 PASS/FAIL 对账）
  examples/attribution_real.rs
apps/desktop/src-tauri/       # Tauri 壳
  src/lib.rs                  #   AppState(db+scans+backfill+instance_id) · setup · RunEvent::Exit 取消钩子
  src/commands.rs             #   M0/M1 命令
  src/commands_m2.rs          #   M2 命令（ToolPath 增加 path_id）
  src/commands_m3.rs          #   M3-A 命令 10 个：授权开关/撤销/卷列表/采集 start/status/cancel/
                              #   来源详情/确认/撤销（单任务约束，后台线程独立连接）
  tauri.conf.json             #   productName FMap · identifier io.github.tracenest.desktop（严禁改）
apps/desktop/ui/              # React 前端（Tailwind v4）
  src/lib/ipc.ts              #   invoke 封装 + 全部类型（含 M3-A 十命令）
  src/lib/format.ts           #   formatBytes + 路由 + sourceBehaviorLabel/sourceActionLabel/eventKindLabel
  src/components/Layout.tsx   #   侧栏 9 页导航 + 顶栏后端状态
  src/pages/Overview.tsx      #   总览：系统状态 + 磁盘列表 + ScanCard + IdentityBackfillCard（采集）
                              #   + 统计卡 10s 自动刷新
  src/pages/Tools.tsx         #   识别中心：归属总览/详情 + Top 路径「来源」面板（确认/修正/撤销/时间线）
  src/pages/Settings.tsx      #   设置：身份采集授权卡（授权/暂停/撤销两模式）+ 隐私承诺
  src/pages/Placeholders.tsx  #   磁盘地图/文件浏览等占位
  src/__tests__/              #   vitest 9 项（format 6 + ipc 3）
```

### 关键机制（新会话必须理解）

**身份回填**（identity_pass.rs）：`run_identity_backfill(db, volume_id, root, cancel, batch_size, on_progress)`
= 授权门禁（双保险）→ 卷 GUID 解析（GetVolumePathNameW → GetVolumeNameForVolumeMountPointW，
任意路径可用）→ fs_type 判定（仅 NTFS 走 FileId 主身份）→ 最新扫描代 rel→path_id 内存映射
（记忆化链重建，**未命中缓存的完整链必须落缓存**——曾因此全 unmapped）→ DFS 身份枚举
（排除清单与扫描引擎一致，重解析点不深入，单目录失败计 denied 继续）→ 批量事务 upsert
（首见 INSERT 不写 discovered 事件——发现事件归 M3-B；重见 UPDATE last_seen；校验位变化
→ 重用嫌疑事件）→ 集合式 `link_attributions_to_entities`（幂等只填 NULL）。

**三层分模型**（source.rs）：用户确认 = 证据(user_statement) → 确认行(append-only，
首认 confirm/再改 correct/revoke→unknown) → 事件（source_confirmed/corrected，引用证据 id）。
有效结论 = 最新一条确认。候选表与确认表物理隔离（测试保证删候选不影响确认）。

**采集进度/取消**：commands_m3 单任务（state.backfill），后台线程独立 Database::open 连接
（与扫描同模式），进度轮询 identity_backfill_status；应用退出 RunEvent::Exit 置位全部取消标志。

**迁移机制**（lib.rs migrate()）：阶段事务 + 逐列幂等 + 崩溃续跑。当前 v5：
v1 基线（schema.sql）→ v2/v3/v4 补列 → v5 = schema_v5.sql 五表 + volume_guid + entity_id。

**前端语义红线文案**：工具关联 ≠ 文件来源；来源行为只能用户确认，自动分析恒 unknown。

### SQLite Schema v5（v5 新增部分）

| 表 | 用途 | 关键列 |
|---|---|---|
| file_entities | 文件身份 | volume_id, identity_kind('file_id'/'weak'), file_id, check_size/check_mtime(校验位), weak_path, current_path_id, first/last_seen_scan_id, first/last_seen_at, deleted_at；部分唯一索引 (volume_id,file_id) 与 (volume_id,weak_path) |
| source_evidence | 原始证据 | kind(user_statement/tool_event/process/browser_download/scan_diff), origin, collected_at, scope, payload(JSON) |
| file_events | 原始事件 append-only | entity_id(FK CASCADE), ts, kind, evidence_id, scan_id, detail |
| source_candidates | 自动候选可整代替换 | entity_id, behavior, tool_id, confidence, evidence_ids(JSON), generation, created_at |
| source_confirmations | 用户确认 append-only | entity_id, action(confirm/correct/revoke), behavior, note, created_at |
| volumes.volume_guid / attributions.entity_id | 关联补列 | 幂等 ALTER；历史行为 NULL |

M1/M2 的 12 张表（meta/volumes/scans/paths/paths_fts/tools/rule_packs/rules/attributions/agg_tool_size/events/watch_roots）结构不变（仅补列）。

## 4. 已确认的技术决策（不得擅自推翻）

| # | 决策 | 理由 |
|---|---|---|
| 1 | **工具关联 ≠ 文件来源** | 位置≠来源；CLAUDE.md 只证明工具工作过 |
| 2 | **来源行为三层分模型**：事件与证据 → 自动候选 → 用户确认 | 确认最高可信；候选可重算；证据不可覆盖（M3-A 已落地） |
| 3 | **来源行为自动分析恒填 unknown** | 只有官方工具事件/进程可靠归因/用户确认才可产候选（M3-C），确认独立存储 |
| 4 | **UNKNOWN 是合法且永久保留的状态** | 宁可显示未知，不可编造来源 |
| 5 | **时间共现只能证明时间关联** | 时间窗 ≠ 来源行为 |
| 6 | **迁移必须阶段事务 + 逐列幂等** | 版本号与 DDL 同事务；崩溃可续跑（v2→v3 事故教训；v5 沿用） |
| 7 | **文件身份 = 卷 GUID + FileId + 降级链，非纯路径** | 盘符可变；FileId 属于卷，跨卷移动=新身份 |
| 8 | **File ID 不是永久唯一标识** | MFT 记录号删除后可能重用（微软文档警告）→ (size,mtime) 校验位 + 重用嫌疑事件（M3-A 已实现检测） |
| 9 | **扫描/回填只写应用数据库；不触碰用户文件** | 只读承诺（CI 门禁 tests/check-readonly.mjs） |
| 10 | **规则数据与引擎分离**（JSON 数据文件） | 社区适配器扩展入口 |
| 11 | **身份键不含盘符**（卷 GUID + FileId） | 重挂载盘符变化不影响 |
| 12 | **Verified 目录继承仅限目录规则来源** | 信号 Likely 不向下传播 |
| 13 | **单实例扫描假设** | 进程存活检测列 M3+ |
| 14 | **身份采集默认关闭；暂停不删数据；撤销时是否删除由用户明确选择** | 逐项授权（M3-A 已落地此 UX） |
| 15 | **枚举采用 FileIdBothDirectoryInfo（Vista+），不用 FILE_ID_EXTD_DIRECTORY_INFO** | 实测绕开交接遗留 #10 的版本范围风险；真机验证枚举与句柄互证一致 |
| 16 | **存量文件首见不写 FILE_DISCOVERED 事件** | 避免一次性数十万条冗余事件；first_seen 字段承载该信息；事件流由 M3-B 差分产生 |
| 17 | **身份回填是独立命令（非扫描内联）** | 零改动 M1 扫描路径（风险最低）；新扫描代未映射路径重跑回填即可对齐 |

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
13. **文件身份采集默认关闭**（M3-A 已落地：默认关/可暂停/撤销时用户选择是否删数据）
14. **不进行未经授权的后台监控**（M3-A 无任何监视；RDCW/USN 属 M3-B/E 且逐项授权）
15. **不得编造扫描结果、测试结果和文件来源**（宁可显示未知）
16. **不启动 M3-B 及后续开发**（除非用户明确安排）

## 6. 当前遗留问题

| # | 问题 | 严重程度 | 状态 |
|---|---|---|---|
| 1 | 双实例并发扫描：instance_id 方案假设单实例 | 低 | 已文档化边界，M3+ 处理 |
| 2 | ~~总览页统计不自动刷新~~ | — | ✅ 已修（10s 轮询） |
| 3 | ~~应用正常关闭无取消钩子~~ | — | ✅ 已修（RunEvent::Exit） |
| 4 | NSIS 打包产物名（FMap）未验证 | 低 | 打包时验证 |
| 5 | 跨平台（macOS/Linux）未测试 | 中 | M1 后期/M3-E |
| 6 | ZCode 规则置信=likely（本机观察）；豆包 Roaming 侧未命中 | 低 | 待全盘扫描复核 |
| 7 | `cargo test --workspace` 偶被 SAC 拦截新二进制（os error 4551）→ 逐 crate 运行等效 | 环境问题 | 已文档化绕过方式 |
| 8 | scan_writer `flush()` 内 `return` 语义依赖闭包嵌套层级（脆弱但正确） | 低 | 已注释 |
| 9 | ~~FileIdExtdDirectoryInfo 版本范围未实测~~ | — | ✅ 已关闭：改用 Vista+ 即可用的 FileIdBothDirectoryInfo（真机验证，见决策 #15） |
| 10 | ~~五表与身份采集未实现~~ | — | ✅ M3-A 已落地 |
| 11 | **新扫描代产生的路径在重跑回填前无实体映射**（来源确认对未采集路径诚实报错） | 低 | 设计如此；UI 提示"先采集"。M3-B 差分可改为增量对齐 |
| 12 | ReFS 128 位 FileId / exFAT ID 重用行为未实测（均诚实走弱身份） | 低 | M3-E 评估 |
| 13 | **真实库本体尚未升 v5**（副本已验证无损；应用下次启动自动迁移） | 注意 | 用户启动应用时发生，属预期 |
| 14 | file_events 目前仅确认/重用嫌疑事件；发现/修改事件依赖 M3-B | 设计 | M3-B 落地 |
| 15 | 来源确认面板的 onDone 回调暂为空操作（确认后未刷新徽标列表，面板内即时可见） | 低 | 可在 M3-B 顺带加列表刷新 |

## 7. 后续开发路线（M3-B → M3-E）

| 阶段 | 目标 | 依赖 | 状态 |
|---|---|---|---|
| ~~M3-A~~ | ~~文件资产基础模型~~ | — | ✅ **已完成（2026-11-05）** |
| **M3-B1** | 基础事件追踪：扫描差分四类事件+台账（保守语义，缺失≠删除） | M3-A ✅ | ✅ **编码完成（B1-0~B1-3，85+11 测试全绿；副本迁移验证通过）；docs/M3_B_IMPLEMENTATION_HANDOFF.md**——真实库迁移与真机验收待用户授权 |
| **M3-B2** | 可选 RDCW 目录监控（**用户已决定纳入 M3-B**）：逐目录授权/默认关闭/启停撤销；原始通知入 events 表；四层水位 gap 生命周期 | M3-B1 验收 | 设计完成（docs/M3_B_DESIGN.md v6 已入库）；编码待 B1-4 真机验收 |
| **M3-C** | AI 工具行为关联：工具关联+时间关联候选（**不产 AI_CREATED/MODIFIED**）；候选重建机制首次投产 | M3-B | 规划完成（语义已修正） |
| **M3-D** | 普通文件来源：浏览器下载记录集成（可选授权）+ 用户标记 UI | M3-A | 规划完成 |
| **M3-E** | 高级持续追踪：USN Journal（需管理员评估）+ 后台监视 + 跨平台 | M3-B + 用户逐项授权 | 规划完成 |

**下一步**：M3-B 设计审核通过后，按 docs/M3_B_DESIGN.md §14 准入评估执行。
**B1 编码前待确认决策（D1–D6）**：v6 全部条目（file_events 重建 + volume_id/rel_path
两列 + coverage_gaps/scan_scope + 索引 + meta 游标/跳过记录 + revoke 语义精确化 +
deleted_at 弃用注释修正）、单对事件上限 50,000、scope 跳过记录载体（meta 结构化键）、
coverage_gaps 改名与阈值（10,000 截断）、单对差分原子事务（BEGIN IMMEDIATE）。
**B2 编码前待确认（D7）**：重启不自动恢复监控（提示式）/ 撤销时通知数据处置 / v7
events 表 source 前瞻列。

**不能重复开发**：M0/M1/M2/M3-A 全部——已实现且测试通过（62 Rust + 9 前端）。
**必须先征求用户同意**：全盘扫描、安装任何组件、Git 推送、identifier 更改、启动 M3-B 编码。

## 8. 本次任务完整记录（M3-A 会话）

### 任务目标
用户确认两项前置（v5 迁移方案 ✅ / 身份采集"默认关闭+可暂停可撤销" ✅）后，完整实现并验收 M3-A。

### 执行顺序与提交链

```
40be846 feat(m3a): v5 迁移——文件资产五表+卷GUID+归属实体关联（22/22）
feat(m3a): platform 文件身份解析——卷GUID+NTFS FileId 批量枚举（真机验证 9/9）
feat(m3a): 来源模型 API + 身份回填 pass——database 35/35
feat(m3a): IPC 命令层 10 命令 + 应用退出取消钩子
e51a940 feat(m3a): 前端——授权卡/采集卡/来源确认面板（vitest 9/9）
test(m3a): validate-schema 升 v5
feat(m3a): 真实库迁移验收例子 identity_real
（本次）docs(m3a): 交接文档更新——M3-A 完成态
```

### 实测结果（全部真实执行）

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | ✅ **62/62**（core 18 + database 35 + platform 9） |
| `cargo clippy --workspace` | ✅ 0 警告 0 错误 |
| `cargo fmt --all -- --check` | ✅ |
| `npm test` / `npm run build`（含 typecheck） | ✅ **9/9** / ✅ |
| `node tests/validate-schema.mjs` | ✅ 22 表（v1 基线 + v5 五表 + 补列），孤立事件/重复 FileId 均被拒 |
| `node tests/check-readonly.mjs` | ✅ |
| 真机 FileId 验证（Win11 26200） | ✅ 枚举与逐文件句柄互证一致；**枚举终止码实测 ERROR_NO_MORE_FILES(0x12)**（文档预期 NO_MORE_ITEMS，已兼容两者） |
| **真实库副本无损迁移** | ✅ v4→v5：paths 508,032/508,032 · attributions 4,245/4,245（全 unknown）· tools 15/15 · volumes 3/3；v5 五表齐全；二次打开幂等；node 交叉核对一致 |
| 真实库本体 | **未动**（仍是 v4；应用下次启动自动迁移——同一段已验收代码） |

### 开发中发现并修复的缺陷（防复发）
1. rel 映射记忆化链重建**未命中缓存的完整链不落缓存** → 全 unmapped（补兜底分支）
2. 双 progress 结构（本地遍历计数与 flush 实体计数分离未合并）→ 汇总前统一合并
3. 撤销删除外键顺序（attributions.entity_id 未先置 NULL 就删实体）→ 调整删除顺序
4. 排除目录在入栈**之后**判断 → 调整为先排除后入栈
5. 前端确认面板与页面工具函数重名 → 合并

## 9. 给新 ZCode 会话的启动指令

以下内容直接复制到新 ZCode 会话即可：

---

**启动指令（复制以下全部内容）**：

你是 FMap 项目（File Map，AI 文件资产地图）的开发工程师，接手一个已完成 M0/M1/M2/M3-A 的项目。

**第一步**：完整阅读 `docs/ZCODE_SESSION_HANDOFF.md`（本文件）。

**第二步：核实项目状态**
1. `git -C <项目路径> branch --show-current && git log --oneline -8 && git status --short`（main 分支、干净工作区）
2. `cargo test --workspace`（应 **62/62**；若 SAC 拦截新二进制则逐 crate：core 18 / database 35 / platform 9）
3. `node tests/validate-schema.mjs`（应 22 表）+ `node tests/check-readonly.mjs`
4. 只读查看真实库 `%APPDATA%/io.github.tracenest.desktop/tracenest.db`（**应用启动后应为 v5**；若仍 v4 说明用户尚未启动新版本应用——这是预期，不要手动迁移真实库）

**第三步**：下一步是 **M3-B（基础事件追踪）**，设计见 `docs/M2_FINAL_HANDOFF.md` §15/§16/§17。关键源码：`crates/database/src/identity_pass.rs`（回填）、`crates/database/src/source.rs`（来源模型）、`crates/platform/src/identity.rs`（身份枚举）、`apps/desktop/src-tauri/src/commands_m3.rs`（IPC）。

**第四步：核实交接结论**
- 62/62 测试、schema v5、attributions 全 unknown、只读门禁通过
- 真实库副本验收命令：`cargo run -p tracenest-database --example identity_real -- <副本路径>`

**第五步：向用户汇报理解与 M3-B 计划，等待用户安排后再开发**。

**铁律（违反即停下询问用户）**：
1. 不使用子代理（套餐限额）
2. 不自动安装 Rust/MSVC/SDK 或其他系统工具
3. 不删除、移动、重命名或修改用户文件
4. 不读取文件正文、聊天记录、AI 会话数据库或 JSONL
5. 不默认计算 Hash
6. 不关闭 Defender、Smart App Control 或添加安全排除项
7. 不提权、不绕过访问控制
8. 不修改注册表或系统环境变量
9. 不未经授权推送 GitHub 或创建仓库
10. 不擅自更改 identifier `io.github.tracenest.desktop`
11. 不把计划中的功能写成已实现
12. 不为测试通过而伪造结果
13. 单文件交接制：阶段完成更新 `docs/ZCODE_SESSION_HANDOFF.md`
14. M0/M1/M2/M3-A 已完成的功能**不要重复开发**
15. 来源行为自动分析只产候选且与用户确认分开存储；M3-A 现状全部为 unknown
16. 身份采集默认关闭的授权语义不得改动

---

## 附：快速参考

| 场景 | 命令 |
|---|---|
| 跑 Rust 全测试 | `cargo test --workspace` |
| 跑单个 crate 测试 | `cargo test -p tracenest-database` |
| 前端测试 | `cd apps/desktop/ui && npm test` |
| 前端构建 | `cd apps/desktop/ui && npm run build` |
| schema 只读检查 | `node tests/validate-schema.mjs` |
| 只读门禁 | `node tests/check-readonly.mjs` |
| 合成文件树 | `node tests/make-fixture.mjs 200 5` |
| 真实库归属分析 | `cargo run -p tracenest-database --example attribution_real` |
| **真实库迁移验收（对副本！）** | `cargo run -p tracenest-database --example identity_real -- <副本db路径>` |
| 桌面应用开发 | `cd apps/desktop/ui && npm run dev` + `cd ../src-tauri && cargo tauri dev` |

| 环境变量（已持久化） | 值 |
|---|---|
| RUSTUP_HOME | D:\rust\rustup |
| CARGO_HOME | D:\rust\cargo |
| PATH 包含 | D:\rust\cargo\bin |

| 安全门禁 | 位置 |
|---|---|
| 只读检查 | tests/check-readonly.mjs |
| Schema 验证（v5） | tests/validate-schema.mjs |
| 前端测试 | apps/desktop/ui/src/__tests__/ |
| 迁移安全测试 | crates/database/src/lib.rs（v4→v5/半迁移/幂等） |
| 三层分模型测试 | crates/database/src/source.rs（隔离/append-only/撤销） |
