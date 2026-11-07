# FMap 会话交接文档（ZCODE_SESSION_HANDOFF）

> **更新时间**：2026-11-07 · **来源会话**：完成 M3-B1/B2 开发+真机验收+GitHub 发布的会话
> **用途**：新 ZCode 会话无缝接手——只上传/阅读本文件即可完整理解项目、继续开发
> **本文件随每次 ZCode 会话结束而更新（覆盖）**，历史版本在 Git 历史中

---

## 1. 项目基本信息

| 项 | 值 |
|---|---|
| **项目名称** | FMap（AI File Asset Tracker） |
| **产品定位** | 本地运行、隐私优先、完全只读的文件资产追踪工具（Windows 首发） |
| **产品口号** | 让 AI 在电脑上留下的每一个文件，都有迹可循 |
| **GitHub** | https://github.com/Yangxbao/FMap (Public · main · v0.1.0 已打标) |
| **工作区路径** | `D:\.zcode\workspace\draft-b6f1ac71-2534-44de-9e25-045ca2d12394\TraceNest` |
| **技术栈** | Tauri 2 · Rust workspace（core/database/platform 三 crate）· React 19 · TypeScript · Tailwind CSS v4 · SQLite（WAL + FTS5）· vitest |
| **工具链** | Rust 1.99.1 stable-msvc → `D:\rust`（RUSTUP_HOME/CARGO_HOME 已 setx）· MSVC v143 + Windows SDK 26100 → `D:\VS2022BuildTools` · Node v24.21.0 · 本机 Windows 11 26200（25H2）· SAC 强制模式 |
| **Git 身份** | 赵子龙 <3296648312@qq.com> · gh CLI 已登录（账号 Yangxbao） |
| **数据库** | `%APPDATA%\io.github.tracenest.desktop\tracenest.db` · **schema v6** · **identifier 严禁更改** |
| **数据规模** | paths 3,304,106（多代累计）· attributions 4,245（全 unknown）· scan_diff_runs 4 · file_events 227 |

---

## 2. 项目当前完成状态

### 已完成里程碑

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** | 项目骨架（Tauri 2 + Rust workspace 三 crate + React 19 + SQLite） | ✅ |
| **M1** | 只读多磁盘扫描引擎（七态状态机 + 批量事务 110K 行/秒 + 覆盖缺口收集 + 多根独立代） | ✅ |
| **M2** | AI 工具识别（15 工具/29 规则 + 四级证据归属引擎 + 识别中心 UI） | ✅ |
| **M3-A** | 文件身份（卷 GUID + NTFS FileId）+ 来源确认三层分模型 + 实机验收 | ✅ |
| **M3-B1** | 基础文件事件追踪（扫描差分四类事件 + 台账 + 覆盖缺口抑制 + 歧义隔离） | ✅ |
| **M3-B2** | RDCW 实时目录监控（逐目录授权 + 原始通知层 + 溢出对账闭环） | ✅ |
| **M3-C** | AI 工具行为关联 | 📋 设计完成，待后续版本 |
| **M3-D** | 浏览器下载记录 | 📋 设计完成，待后续版本 |
| **M3-E** | USN Journal + 跨平台 | 📋 设计完成，待实施 |

### 当前真实数据库状态（v6 · 应用自身迁移完成）

| 表 | 行数 |
|---|---|
| paths | 3,304,106（多代累计：scan5 E 盘 368K + scan6 全盘 952K + scan7-10 各 ~368K） |
| attributions | 4,245（source_behavior 全 unknown） |
| tools | 15 |
| volumes | 5（C:\ D:\ E:\ + E:\docs + E:\fmap-e2e 测试目录） |
| scans | 10（含历史 failed/cancelled + v6 后 6 个 done） |
| scan_diff_runs | 4（全 completed：to_scan 7/8/9/10） |
| file_events | 227（discovered 133 / modified 89 / missing 4 / reappeared 1） |
| file_entities | 0（身份采集默认关闭，未采集） |
| watch_roots | 0（监控默认关闭，未授权） |

---

## 3. 代码结构

```
Cargo.toml                        # workspace 根
crates/core/
  src/lib.rs                      # APP_NAME/VERSION · ScanStatus 七态 · CoreError · format_bytes
  src/scanner.rs                  # 扫描引擎：scan_root() 只读遍历 + 覆盖缺口收集 + 排除 + 链接不跟随
  src/attribution.rs              # 归属引擎纯逻辑：evaluate_node() · 四级证据 · 冲突
  src/pathnorm.rs                 # 路径规范化（normalize/normalize_root/祖先前缀/缺口匹配）
  src/scan_scope.rs               # ScanScope 等价比较 + CoverageGaps 解析
crates/platform/
  src/lib.rs                      # FileSystemProvider trait · 磁盘发现 · 提权检测 · 能力声明
  src/identity.rs                 # M3-A 身份解析：卷 GUID + NTFS FileId 批量枚举 + 回退
  src/watcher.rs                  # M3-B2 RDCW 监控（overlapped + 可取消 + 溢出检测 + 零去重）
crates/database/
  src/lib.rs                      # Database::open/migrate(v1→v6) · upsert_volume(规范化) · 查询 DAO
  src/scan_writer.rs              # run_scan_to_db()：遍历→批量事务写→聚合→终态
  src/rules_loader.rs             # 内置规则包 JSON→tools/rule_packs/rules
  src/attribution_pass.rs         # 归属 pass + 识别中心查询 API
  src/source.rs                   # M3-A 来源模型（确认/撤销/域分治）
  src/identity_pass.rs            # M3-A 身份回填 pass
  src/diff_pass.rs                # M3-B1 差分引擎（选对/分类/双事务/台账/缺失集）
  src/watch.rs                    # M3-B2 授权 CRUD/通知持久化/gap 生命周期
  examples/identity_real.rs       # 真实库迁移验收例子
apps/desktop/src-tauri/
  src/lib.rs                      # AppState(db+scans+backfill+watch_handles) · 退出钩子
  src/commands.rs                 # M0/M1 扫描 + spawn_scan_job(统一入口) + M3-B1 IPC
  src/commands_m2.rs              # M2 识别中心 IPC
  src/commands_m3.rs              # M3-A 身份采集/来源确认 IPC
  src/commands_m3b.rs             # M3-B2 监控 IPC（watch_start/pause/remove/list/gap）
apps/desktop/ui/
  src/lib/ipc.ts                  # invoke 封装 + 全部类型
  src/lib/format.ts               # formatBytes + 路由 + 语义标签（来源行为/事件/台账）
  src/components/Layout.tsx       # 侧栏 9 页导航 + 顶栏
  src/pages/Overview.tsx          # 总览：系统状态 + 磁盘 + 扫描卡 + 身份采集卡
  src/pages/Tools.tsx             # AI 工具识别中心 + 来源确认面板
  src/pages/History.tsx           # 历史追踪页（差分台账 + 事件时间线）
  src/pages/Settings.tsx          # 设置页（身份采集授权卡 + 监控授权卡）
tests/
  schema.sql                      # v1 基线 DDL
  schema_v5.sql                   # v5 增量（M3-A 五表）
  schema_v6.sql                   # v6 增量（M3-B1 台账 + file_events 重建）
  check-readonly.mjs              # 只读安全门禁
  validate-schema.mjs             # Schema 验证（v1+v5+v6+补列 → 23 表）
  make-fixture.mjs                # 合成文件树生成器
```

---

## 4. 关键设计决策（不得擅自推翻）

| # | 决策 | 理由 |
|---|---|---|
| 1 | **工具关联 ≠ 文件来源** | 位置≠来源；CLAUDE.md 只证明工具工作过 |
| 2 | **三层分模型**：事件与证据→自动候选→用户确认 | 确认最高可信；候选可重算；证据不可覆盖 |
| 3 | **自动分析恒填 UNKNOWN** | 只有用户确认才产来源行为 |
| 4 | **UNKNOWN 永久合法** | 宁可显示未知，不可编造来源 |
| 5 | **时间共现 ≠ 来源行为** | 时间窗只能证明时间关联 |
| 6 | **迁移=阶段事务+逐列幂等+崩溃续跑** | 版本号与 DDL 同事务 |
| 7 | **文件身份=卷 GUID+FileId+降级链** | 盘符可变；跨卷移动=新身份 |
| 8 | **FileId 不是永久唯一** | MFT 重用→(size,mtime) 校验位+重用嫌疑事件 |
| 9 | **只写应用数据库；不碰用户文件** | 只读承诺（CI 门禁） |
| 10 | **规则数据与引擎分离** | JSON 数据文件 |
| 11 | **身份键不含盘符** | 用卷 GUID/卷序列号 |
| 12 | **Verified 继承仅限目录规则** | 信号 Likely 不向下传播 |
| 13 | **身份采集默认关闭** | 暂停不删数据；撤销时用户选择是否删 |
| 14 | **枚举用 FileIdBothDirectoryInfo（Vista+）** | 实测绕开 FILE_ID_EXTD_DIRECTORY_INFO 版本风险 |
| 15 | **首见实体不写 FILE_DISCOVERED** | 避免 50 万条冗余；事件由 M3-B 差分产生 |
| 16 | **身份回填是独立命令** | 零改动 M1 扫描路径 |
| 17 | **多根=每根独立扫描代** | 差分卷隔离；一卷失败不污染他卷 |
| 18 | **MISSING≠DELETED** | 缺失=观察陈述（可逆）；覆盖不确定宁漏勿误 |
| 19 | **采集层零去重** | UI 折叠展示，原始数据完整保留 |
| 20 | **身份域/观察域分离** | 撤销身份采集只删身份域事件 |

---

## 5. 安全红线（完整记录，违反即停下）

1. **不删除、移动、重命名或修改用户原有文件**——只读承诺；CI 门禁 `tests/check-readonly.mjs`
2. **不读取文件正文作为默认功能**（默认功能只用元数据）
3. **不默认计算 Hash**
4. **不读取聊天记录、AI 会话数据库和 JSONL**
5. **不擅自提权或绕过访问控制**
6. **不关闭 Defender、Smart App Control 等安全机制**（本机 SAC = 强制模式 0x1）
7. **不添加 Defender 排除项**
8. **不自动安装工具链或修改系统配置**
9. **不未经授权推送 GitHub**（远程已配置 https://github.com/Yangxbao/FMap）
10. **不使用子代理**——用户体验套餐限额；**一切工作主会话直接做**
11. **不擅自迁移项目目录**（工作区路径保持）
12. **隐私敏感功能默认关闭，必须由用户主动授权**
13. **不进行未经授权的后台监控**
14. **不得编造扫描结果、测试结果和文件来源**
15. **不启动 M3-C/D/E 开发**（除非用户明确安排）
16. **identifier `io.github.tracenest.desktop` 严禁更改**

---

## 6. 当前遗留问题

| # | 问题 | 严重程度 | 状态 |
|---|---|---|---|
| 1 | **多根扫描混挂历史数据**：scan6（C+D+E 全盘）952K 路径混挂卷 2 | 低 | 差分防御正确拒绝；C 盘重扫即恢复；根治=已完成（每根独立代） |
| 2 | **真实库需两次新扫描产生首个差分对**：历史 done 代无 scope | 设计 | scan7-10 已有 scope；新扫描差分正常运行 |
| 3 | rel_key 与 NTFS $UpCase 极端 Unicode 理论差异 | 低 | 歧义隔离兜底（碰撞即隔离不合并） |
| 4 | run_diff_now 同步命令 | 低 | B1 最小；异步留 B2+ |
| 5 | 历史页事件上限 300 条 | 低 | 分页留 B2+ |
| 6 | E 盘扫描耗时波动（14.7s→4-5min） | 低 | 非正确性；杀毒/索引干扰推测 |
| 7 | UI 二次点击 checkbox 取消勾选 → "未选=全盘"默认 | 中 | 已修（未选时禁用按钮）；交互边界仍需注意 |
| 8 | 侧栏"历史追踪"徽标已改 B1（本轮完成） | — | ✅ 已修 |
| 9 | NSIS 安装包未验证 | NOT RUN | tauri build 需网络下载 NSIS 工具链 |
| 10 | M3-C/D/E 未实现 | 设计完成 | 非核心追踪功能；待后续版本 |
| 11 | macOS/Linux | 代码边界清晰 | 需平台测试 |

---

## 7. Git/GitHub 状态

| 项 | 值 |
|---|---|
| 分支 | main |
| HEAD | `e08b40e`（最终报告提交） |
| 远程 | `https://github.com/Yangxbao/FMap.git`（已配置并已推送） |
| 可见性 | Public |
| 标签 | v0.1.0 |
| gh CLI | 已登录（账号 Yangxbao） |
| 工作区 | 干净（最终报告已推送） |

---

## 8. 数据库 Schema v6（全部 23 表）

| 表 | 用途 | 关键列 |
|---|---|---|
| meta | schema_version/授权/游标/gap | key,value |
| volumes | 卷登记（root UNIQUE · 规范化存储） | root, fs_type, volume_guid |
| scans | 扫描代（七态） | roots, scan_scope, coverage_gaps, files_scanned |
| paths | 路径节点（按代追加） | volume_id, parent_id, name, size, mtime, deleted_at |
| paths_fts | 文件名全文搜索 | name |
| tools | AI 工具（15） | key UNIQUE, name, kind |
| rule_packs | 规则包 | pack_name, version |
| rules | 规则（29 enabled） | tool_id, kind(dir/signal), pattern, weight |
| attributions | 归属结论 | path_id, tool_id, confidence, source_behavior(全 unknown), entity_id |
| agg_tool_size | 目录×工具聚合 | path_id, tool_id, direct_bytes, total_bytes |
| events | 原始通知层（M3-B2 RDCW 写入；M5 预留） | ts, kind, volume_id, path, watch_root |
| watch_roots | 监控授权（path UNIQUE 规范化） | path, enabled, added_at |
| file_entities | 文件身份（M3-A） | volume_id, identity_kind, file_id, check_size/mtime, current_path_id |
| file_events | 差分+确认事件（append-only） | entity_id(可空), path_id(可空), volume_id, rel_path, rel_key, ts, kind, diff_run_id |
| source_evidence | 来源证据 | kind, origin, payload(JSON) |
| source_candidates | 自动候选（可重建） | entity_id, behavior, generation |
| source_confirmations | 用户确认（append-only） | entity_id, action, behavior |
| scan_diff_runs | 差分执行台账 | volume_id, from/to_scan_id, status(四态), events_count, ambiguous_count |
| + FTS5 影子表 4 张 | paths_fts 内部 | — |

**file_events 挂点**：entity_id 或 path_id 至少其一（表级 CHECK）；差分事件带 volume_id+rel_key（逻辑路径键）+ diff_run_id（台账强关联）。

---

## 9. 关键设计决策与已知限制

### 设计决策（完整 20 条见 §4；此处补充 B1/B2 新增）

| 决策 | 说明 |
|---|---|
| 差分选对=done 代升序+台账连续前缀+pending 阻塞 | 保证事件插入严格按代序；缺失集推导无乱序 |
| 双事务：正常差分（事件+completed 台账原子）/ 超限状态事务（独立） | pending 不会被差分回滚连带 |
| 条件 UPSERT（仅覆盖 pending） | completed 重放=no-op 幂等 |
| 缺失集两步查询（歧义排除+最新缺失） | SQLite 窗口函数不支持 COUNT(DISTINCT) |
| coverage_gaps 统一相对登记根坐标系 | 防绝对/相对混比 |
| 多根=每根独立扫描代 | 修复 M1 混挂；差分卷隔离 |
| 溢出=句柄保持继续消费+自动对账 | 不关句柄不丢后续通知 |
| 采集层零去重 | UI 折叠展示，原始数据完整 |
| 身份域/观察域分离 | 撤销身份采集只删身份域事件 |
| B2 events 表零结构变更 | USN 留 M3-E 正式设计 |

### 已知限制

- 真实库历史 done 代（scan5 前）无 scope → 首个差分对需两次新扫描
- rel_key 与 NTFS $UpCase 极端 Unicode 理论差异（歧义隔离兜底）
- run_diff_now 同步命令（扫描进行中会等待写锁）
- 历史页事件上限 300 条
- E 盘扫描耗时波动（14.7s→4-5min，杀毒/索引干扰推测）
- 多根输入仅 roots[0] 被扫描的 M1 边界已修复（每根独立代）但历史数据需 C 盘重扫覆盖

---

## 10. 测试基线

| 套件 | 数量 | 状态 |
|---|---|---|
| cargo test --workspace | **93/93** | ✅（core 25 · database 56 · platform 12） |
| cargo clippy --workspace | 0 error / 1 minor | ✅ |
| cargo fmt --check | ✅ | |
| npm test（前端 vitest） | **11/11** | ✅ |
| npm run build（含 tsc） | ✅ | |
| node tests/check-readonly.mjs | ✅ | |
| node tests/validate-schema.mjs | ✅ 23 表 | |

---

## 11. 快速参考

| 场景 | 命令 |
|---|---|
| Rust 全测试 | `cargo test --workspace` |
| 单 crate 测试 | `cargo test -p tracenest-database` |
| 前端测试 | `cd apps/desktop/ui && npm test` |
| 前端构建 | `cd apps/desktop/ui && npm run build` |
| Schema 验证 | `node tests/validate-schema.mjs` |
| 只读门禁 | `node tests/check-readonly.mjs` |
| 真实库迁移验收（对副本！） | `cargo run -p tracenest-database --example identity_real -- <副本>` |
| 桌面应用开发 | `cd apps/desktop/ui && npm run dev` + `cargo tauri dev` |
| release 构建 | `npm run build --prefix apps/desktop/ui && cargo build --release -p tracenest-desktop --features custom-protocol` |

| 环境变量（已持久化） | 值 |
|---|---|
| RUSTUP_HOME | D:\rust\rustup |
| CARGO_HOME | D:\rust\cargo |
| PATH 包含 | D:\rust\cargo\bin |

---

## 12. 下一步开发路线

| 优先级 | 任务 | 前置条件 |
|---|---|---|
| 1 | **真机验收 B2**：启动应用→设置页添加监控目录→验证通知采集/gap 生命周期→历史页原始通知展示 | 用户安排 |
| 2 | **NSIS 安装包**：`cargo tauri build`（需网络下载 NSIS 工具链）→ 安装/卸载/升级测试 | 网络 |
| 3 | **M3-C**：AI 工具行为关联（时间关联候选；不产 AI_CREATED） | B2 稳定 |
| 4 | **历史页增强**：分页/按路径搜索/卷过滤 | 低优先 |
| 5 | **超限处置 UI**：pending_over_limit 的提高上限/放弃入口 | 低优先 |
| 6 | **多根扫描拆分**（如需）：C 盘重扫覆盖混挂数据 | 用户需要 C 盘差分时 |
| 7 | **M3-D/E**：浏览器记录/USN | 远期 |

---

## 13. 给新 ZCode 会话的启动指令

以下内容直接复制到新 ZCode 会话即可：

---

**启动指令**：

你是 FMap 项目（AI File Asset Tracker）的开发工程师，接手一个已完成 M0/M1/M2/M3-A/M3-B1/M3-B2 并发布 v0.1.0 到 GitHub 的项目。

**第一步**：阅读本文件（`docs/ZCODE_SESSION_HANDOFF.md`），理解项目全貌。

**第二步**：核实项目状态
1. `git log --oneline -5 && git status --short`（确认 main、干净工作区）
2. `cargo test --workspace`（应 **93/93**）
3. `node tests/validate-schema.mjs`（应 23 表）+ `node tests/check-readonly.mjs`
4. 只读查看真实库：`schema_version=6`、`paths≈3.3M`、`scan_diff_runs=4`

**第三步**：根据用户指令决定下一步。可能的方向：
- B2 真机验收（启动应用→设置页授权监控→验证通知采集）
- NSIS 安装包构建
- M3-C 开发
- 历史页增强

**铁律**：
1. 不使用子代理
2. 不自动安装系统组件
3. 不删除/修改用户文件
4. 不读文件正文/聊天记录
5. 不关闭 Defender/绕过权限
6. 不编造测试结果
7. 来源行为只能用户确认；自动分析恒 UNKNOWN
8. 身份采集/监控默认关闭
9. identifier 严禁更改
10. 不 push 除非用户安排
11. 单文件交接制：阶段完成更新本文件
