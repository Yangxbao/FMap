# FMap M3-B1 完成报告（提交 GPT 审核用 · 自包含全文）

> **报告日期**：2026-11-07 · **作者**：ZCode（主会话实现，无子代理）
> **性质**：B1（基础文件事件追踪）完成报告 + 请审核事项
> **阅读对象**：ChatGPT（此前已审阅本设计 v1–v5；v6 与实现结果以本报告为准）
> **本报告自包含**：所有背景、设计决策、实现细节、测试证据均已内嵌，无需访问本机文件

---

## 一、项目背景（供对齐上下文）

**FMap**（原 TraceNest）：本地运行、Windows 首发、**只读**的 AI 文件资产追踪工具。
技术栈：Tauri 2 + Rust workspace（core/database/platform 三 crate）+ React 19 + SQLite
（rusqlite bundled, WAL, FTS5）。数据位于 `%APPDATA%\io.github.tracenest.desktop\tracenest.db`。

**已完成里程碑**：M0（骨架）/ M1（只读扫描引擎，批量事务 110K 行/秒）/ M2（AI 工具规则库
15 工具 29 规则 + 归属引擎四级证据 + 识别中心 UI）/ M3-A（文件身份：卷 GUID + NTFS
FileId + 来源确认三层分模型）/ **M3-B1（本报告：基础文件事件追踪）**。

**不可推翻的用户红线（摘要）**：
1. 只读承诺——永不删除/修改/移动/重命名用户文件（CI 门禁 `tests/check-readonly.mjs`）；
2. 不读文件正文、不默认算 Hash、不读 AI 聊天记录/会话库；
3. 隐私功能默认关闭、逐项授权；不进行未经授权的后台监控；
4. **工具关联 ≠ 文件来源**；来源行为（AI_CREATED 等）只能由用户确认产生，
   自动分析恒为 UNKNOWN；UNKNOWN 永久合法；
5. 身份采集默认关闭；扫描差分**不需要**身份采集授权（纯路径元数据）；
6. 不使用子代理；不推送 GitHub（无远程仓库）；真实库迁移需用户授权。

**用户已定的 M3-B 决策（不可再议）**：RDCW 实时目录监控纳入 M3-B（B1 验收通过后
实施）；B1（扫描差分）先于 B2（RDCW）；FILE_MISSING 保守策略（覆盖不确定宁漏勿误）；
FILE_DELETED 不允许由扫描缺失或 RDCW 通知直接认定。

---

## 二、B1 设计定稿要点（v6，含 v5→v6 演进原因）

设计经五轮迭代（v1→v6），关键定稿与演进理由如下（供审核判断设计合理性）：

### 2.1 逻辑路径身份
- 逻辑路径键 = `(volume_id, rel_key)`；`rel_key` = 相对**卷登记根**路径经
  `to_uppercase()` + 分隔符统一 '/' + 去尾分隔符。
- **放弃 SQLite `COLLATE NOCASE`**：它只做 ASCII 折叠，≠ Windows/NTFS 完整
  Unicode 大小写语义。
- **Unicode 碰撞防御（歧义隔离）**：`ß.txt` 与 `SS.txt` 的 to_uppercase 同键——
  同键出现多个不同原样 rel_path 时判**歧义**：四类事件全跳过、不参与缺失集、
  不参与正常分类（宁可无事件，不错误合并不同文件）；歧义是**判定时性质不持久化**
  （重检测零成本），台账以 `ambiguous_count` 轻量计数。ASCII/中文场景零影响。

### 2.2 差分对选择与台账（状态机）
- 选对：该卷 `status='done'` 扫描代升序 G=[g₁..g_k]；**相邻对** = G 中直接后继；
  failed/cancelled/access_denied/volume_offline 代不参与（非完整快照）；
  **不跳对**（跳对会丢失中间代"出现又消失"文件的完整轨迹）。
- 台账表 `scan_diff_runs`：`UNIQUE(volume_id, to_scan_id)`；状态四种：
  - `completed`（含**零事件对**——游标推进的可追溯依据）/ `skipped_scope_mismatch` /
    `skipped_over_limit` = 已处理（允许后续继续）；
  - `pending_over_limit` = **阻塞**：该对及后续全部停止（保证事件插入严格按代序，
    缺失集"每键最新事件"推导不被旧差分的晚插入破坏）。
- 派生游标 = 连续已处理前缀末端；**不变式**：台账行必须构成基线起的连续前缀
  （有校验函数）。**completed/skipped 历史行禁止重开**（append-only）；
  pending 行可人工改判（→skipped_over_limit 或删除行提高上限重跑）。
- **条件 UPSERT**：completed 台账 `ON CONFLICT ... DO UPDATE ... WHERE status=
  'pending_over_limit'`——仅允许覆盖 pending 行（提高上限重跑自动恢复）；
  重放 completed 对 = 全部 no-op（幂等）。**坑**：UPSERT 走 UPDATE 分支时
  `last_insert_rowid()` 不更新——已改为按唯一键显式 SELECT id。

### 2.3 双事务模型
- **正常差分事务**（BEGIN IMMEDIATE）：事件（INSERT OR IGNORE，唯一索引
  `(kind, path_id, scan_id)` 兜底）+ completed 台账行，原子提交；
- **超限状态事务**（独立）：仅写 pending_over_limit 台账单行——与差分事务分离，
  不会被差分回滚连带；写入失败时超限事实不丢（事件量每次重算，下次触发重建
  pending 并上报错误）；
- **上限**：50,000 事件/对（可配置参数）；事务前内存计数 + 事务内 Rust 侧逐条
  累计双道检查（SQLite `changes()` 只反映最近一条语句，不可用于事务累计）。

### 2.4 覆盖完整性与四事件语义
- scanner 五类跳过点收集覆盖缺口（目录枚举失败/条目失败/类型失败/metadata 失败/
  排除目录），相对登记根规范化存储，上限 10,000 截断 + `truncated` 标记；
- **扫描范围快照** `scans.scan_scope`（roots+排除规则+符号链接策略）：差分前置校验
  两代 scope 规范化一致，**不一致或缺失 → 整对跳过**（原因入台账可追溯）；
- **匹配方向（v4 修正）**：目录缺口=自身+子树（逐分量祖先查找）、文件缺口=仅自身、
  未知类别保守归目录级；**分量边界**保证 `D:\proj` 不误伤 `D:\project2`；
- 四事件产生条件（含覆盖完整性）：
  - **file_discovered**「首次观察到」：rel∈S_curr、∉S_prev、∉缺失集、不在 S_curr
    缺口内；S_prev 缺口/截断覆盖时 detail 标 `prev_gap_covered/unknown`
    （**首次观察到 ≠ 刚刚创建**——UI 文案硬规范）；
  - **file_modified**「观察到变化」：双代均观察到 + (size,mtime) 变化且非 NULL；
    detail 存前后值 + "不代表内容变化"；
  - **file_missing**「缺失（待确认）」：rel∈S_prev、∉S_curr、S_curr 覆盖完整
    （未截断 + 候选不在缺口内）；**S_prev 缺口不影响**（缺口只会漏报不会虚报存在）；
    被抑制的候选计入 `missing_suppressed`（宁漏勿误的可见证据）；
  - **file_reappeared**「重新出现」：∈S_curr、∉S_prev、∈缺失集；
    detail 带 `missing_since_scan`（=缺失期首次未观察到的扫描代）+"原因不做推断"；
- **file_missing ≠ file_deleted**：缺失=观察陈述（可逆，重现→reappeared）；
  deleted=事实断言（需删除事件流/用户确认，M3-B **无产生路径**，测试断言恒为 0）；
- **缺失集推导**：SQL 窗口函数 `PARTITION BY rel_key ORDER BY id DESC` 取每键最新
  差分事件（kind 白名单四类），最新为 file_missing 者入集；**两步查询**——SQLite
  窗口函数不支持 `COUNT(DISTINCT)`，歧义键单独 GROUP BY HAVING 后在 Rust 侧相减；
  事件插入严格按代序（阻塞保证），推导无乱序风险。

### 2.5 撤销身份采集的域分治
- **身份域事件**（source_confirmed/source_corrected/file_id_reuse_suspected，按
  `IDENTITY_DOMAIN_KINDS` 注册表）撤销时删除；**观察域事件**（差分四类）与
  **台账**永久保留（先删身份域**再**解除观察域 entity 引用——顺序错误会触发
  表级 CHECK，真机开发中实际踩过并修复）；孤儿 user_statement 证据清理；
- `scan_diff_runs` 属观察域，撤销完全不触碰；
- 表级 CHECK `(entity_id IS NOT NULL OR path_id IS NOT NULL)`——CREATE TABLE
  时可引用同行任意列（ADD COLUMN 不行），重建路径下直接生效。

---

## 三、实现结构（当前代码）

```
crates/core/src/pathnorm.rs        # 统一路径规范化 + 分量边界缺口匹配（三消费方共用）
crates/core/src/scan_scope.rs      # ScanScope 等价比较 + CoverageGaps 解析
crates/core/src/scanner.rs         # 扫描引擎 + 五类覆盖缺口收集（上限 10000 截断）
crates/database/src/lib.rs         # v6 迁移 + upsert_volume 登记规范化 + 查询 DAO
crates/database/src/diff_pass.rs   # 差分引擎（选对/分类/双事务/台账/缺失集）
crates/database/src/source.rs      # 撤销域分治 + kind 域注册表
apps/desktop/src-tauri/src/commands.rs  # scope/coverage 落库 + done 自动差分 + 3 IPC
apps/desktop/ui/src/pages/History.tsx   # 历史追踪页（台账+事件时间线）
tests/schema_v6.sql                # v6 增量 DDL（迁移同源）
```

**接线方式**：`scan_start` 后台线程 → begin_scan 后写 scope 快照 → 扫描 done 后
coverage_gaps 落库 → **自动触发差分**（同线程同连接；失败不影响扫描终态，stderr
记录结果）。

---

## 四、测试与验证结果（全部真实执行，无虚构）

### 4.1 自动化测试
| 套件 | 结果 |
|---|---|
| `cargo test --workspace` | ✅ **85/85**（core 25 · database 51 · platform 9）exit 0 |
| database 新增专项 | v6 迁移（v5→v6 含非空 file_events 搬运、脏残留收敛、约束验证）、diff_pass 9 项真机多代场景、T50 卷登记规范化、T33 域完备性、撤销观察域保留 |
| `cargo clippy --workspace` | ✅ 0 警告 |
| `cargo fmt --check` | ✅ |
| `npm test`（前端 vitest） | ✅ **11/11**（含语义红线断言：事件文案不含"创建""删除"） |
| `npm run build`（含 tsc） | ✅ |
| `node tests/validate-schema.mjs` | ✅ 23 表；CHECK/唯一索引/台账 UNIQUE 验证拒绝 |
| `node tests/check-readonly.mjs` | ✅（精确化：跳过 #[cfg(test)] 测试模块——门禁意图=保护用户文件） |

### 4.2 真实库副本迁移验证（迁移真实库之前）
- 备份三件套（v5 · `PRAGMA integrity_check`=ok）→ 对副本执行迁移 →
  **v6 · paths 508,032/508,032 · attributions 4,245/4,245（全 unknown）·
  tools 15/15 · volumes 3/3** · node:sqlite 交叉核对一致。

### 4.3 真机端到端验收（用户授权后，2026-11-06）
**环境**：release 构建（补 Tauri 2 `custom-protocol` feature，内嵌前端资产）·
普通用户权限 · 真实 E 盘（单次扫描 ~368K 路径，耗时 4–5 分钟）·
测试文件仅建于自建目录 `E:\fmap-e2e\`。

| 轮 | 操作 | 差分对 | 结果 |
|---|---|---|---|
| 1 | 基线扫描 | （历史 4 代均 failed/cancelled，无 done 前驱）| **基线语义正确**：无差分对，日志"事件 0" |
| 2 | 意外全盘扫描（C+D+E，952K 路径）| — | **卷隔离防御正确**：混挂代（paths 全记到第一根的卷）与 E 盘基线不构成合法对，拒绝研判——设计防御经受真实考验 |
| 3 | 新建 b.txt → 单盘 E 扫描 | (5,7) completed **90 事件** | ✅ **DISCOVERED** `fmap-e2e/b.txt`（含 E 盘真实变化 60 新/30 改）|
| 4 | 修改 a.txt（18B→64B）→ 扫描 | (7,8) completed **90 事件** | ✅ **MODIFIED** detail：`{"from":{"mtime":...,"size":18},"to":{...,"size":64},"note":"观察到的元数据变化，不代表内容变化"}` |
| 5 | 删除 b.txt → 扫描 | (8,9) completed **12 事件** | ✅ **MISSING** `scan_id=9`（缺失起始代）detail："待确认，不代表已删除"；**file_deleted = 0** |
| 6 | 恢复 b.txt → 扫描 | (9,10) completed **35 事件** | ✅ **REAPPEARED** detail：`{"missing_since_scan":9,"note":"...原因不做推断"}` |

- **台账**：4 行连续 completed（to_scan_id 7/8/9/10）——区间连续无跳对；
- **事件总量**：227 条（discovered 133 / modified 89 / missing 4 / reappeared 1，
  含 E 盘两次扫描间的真实系统变化）；
- **UI 一致性**：历史追踪页（`#/history`）事件行与数据库逐字段一致
  （时间/类型徽标/卷号/相对路径/detail 摘要/语义文案"首次被完整扫描观察到，
  不代表文件刚刚创建"）；
- **退出钩子**：stderr 实录 6 条差分结果 + "退出：已请求取消全部在跑任务"，
  应用优雅退出（exit 0）；
- **真实库迁移**：应用自身完成 v5→v6，迁移后 node 交叉核对：schema v6 ·
  paths 3,304,106（多代累计，代式追加存储）· attributions 4,245 全 unknown ·
  台账 4 行 · 事件 227 · scans_running=0。

---

## 五、过程中发现并修复的缺陷（防复发）

1. **撤销顺序违反 CHECK**（真机开发发现）：若先解除 entity 引用，entity-only 的
   身份域事件失去唯一挂点、违反表级 CHECK——修正为**先删身份域事件、再解除观察域
   引用**。
2. **UPSERT 分支 last_insert_rowid 陷阱**：条件 UPSERT 走 UPDATE 时 rowid 不更新，
   diff_run_id 拿到旧值导致 FK 失败——改为按唯一键显式 SELECT id。
3. **SQLite 窗口函数不支持 COUNT(DISTINCT)**——缺失集歧义检测拆两步查询。
4. **v4→v5 设计演进中的历史问题**（设计审核轮已修，实现前落地）：多批事务部分
   提交导致分类漂移（改原子对事务）；按 path_id 推导缺失集错误（改逻辑路径键）；
   coverage 匹配方向反了（改逐分量祖先）；`changes()` 非事务累计（改 Rust 侧累计）。
5. **[M0 骨架缺陷] 缺 `custom-protocol` feature**：src-tauri/Cargo.toml 补
   `[features] custom-protocol = ["tauri/custom-protocol"]`——release 构建现内嵌
   前端资产（`tauri.localhost`），不再依赖 dev server。

## 六、真机发现的新问题（已记录，未阻塞 B1）

1. **[M1 既有边界暴露] 多根扫描数据混挂**：一次意外全盘扫描（C+D+E）的全部路径
   挂到第一根的卷（C 盘，952K 行）——**差分卷隔离防御正确拒绝**了非法对（scan5(E)
   与混挂代不构成对），但混挂数据已在库中。**影响**：C 卷后续差分被保守跳过；
   **缓解**：C 盘重新单盘扫描即恢复正常序列；**根治**：多根扫描拆分为每卷独立
   扫描代（计划 M3-C 前）。
2. **E 盘扫描耗时波动**：同盘两次扫描 14.7s（M3-A）→ 4–5 分钟（本轮，368K 路径
   + 杀毒/索引干扰推测）——性能基线需重新测定（非正确性问题）。
3. UI 二次点击 checkbox 会取消勾选，且"未选=全盘"默认在该场景易误触发——
   已知交互边界，未改（B1 范围外）。

## 七、已知限制（B1 范围内接受，记录为后续优化）

1. 真实库升级 v6 后需**两次新扫描**产生首个差分对（历史代无 scope，保守跳过）；
2. rel_key 与 NTFS $UpCase 的极端 Unicode 理论差异——**歧义隔离**兜底（碰撞即
   隔离，不合并）；
3. `run_diff_now` 同步命令（可能等待写锁）；超限处置为"补跑"按钮 + 编译期常量；
4. 历史页事件上限 300 条（分页留 B2+）；侧栏"历史追踪"徽标仍为 V0.2（cosmetic）；
5. 多根输入仅 roots[0] 被扫描（M1 边界）；
6. 测试目录 `E:\fmap-e2e\` 保留供复查（自建文件，可随时人工删除）。

## 八、用户决策（D1–D10）落实情况

| 决策 | 内容 | 状态 |
|---|---|---|
| D1 | v6 全部条目（台账/file_events 重建+volume_id+rel_key/diff_run_id+scope/coverage 列/CHECK/volume 登记规范化） | ✅ 已实现并迁移 |
| D2 | 上限 50,000 + 阻塞策略 + 双事务 | ✅ 已实现 |
| D3 | revoke 按 kind 域删除 | ✅ 已实现 |
| D4 | 台账替代 meta 跳过键 + diff_run_id 强关联 | ✅ 已实现 |
| D5 | coverage_gaps 改名/阈值 10000/相对坐标系 | ✅ 已实现 |
| D6 | deleted_at 弃用追认 | ✅ 已落实（注释修正随 v6 入库） |
| D7-a/b | B2 重启提示式恢复 / 撤销通知处置 | ⏳ B2 待定 |
| D8 | DISCOVERED 产生+prev_gap 标记 | ✅ 已实现（真机验证） |
| D9 | rel_key 歧义隔离 | ✅ 已实现（真机验证 ß/SS 碰撞由单测覆盖，真机场景未见碰撞——歧义计数路径由单测断言） |
| D10 | 溢出四层水位判据 | ⏳ B2 范围 |

## 九、请 GPT 重点审核的事项

1. **端到端证据充分性**：§4.3 的四类事件真机链路（含 detail 原文）是否足以确认
   B1 达成"可用、可验证的基础文件历史追踪"目标？
2. **多根混挂问题的处置**：本轮以"卷隔离防御 + C 盘重扫恢复"处理是否可接受？
   是否要求在 B2 前先完成"多根拆分独立扫描代"？
3. **已知限制清单**（§6）是否有应升级为阻断项的条目？
4. **B2 启动条件复核**：B1-4 真机验收已通过；B2（RDCW）按设计 v6 §7 实施
   （watch_roots 授权/events 原始通知层/四层水位 gap 生命周期/溢出对账闭环/
   采集层零去重）——设计是否需在编码前补充澄清？

## 十、B2 计划摘要（下一阶段，待用户/GPT 确认后启动）

- **范围**：RDCW 可选目录监控——watch_roots 逐目录授权（默认关闭、启动/暂停/
  恢复/撤销、嵌套运行时去重）、原始通知入 `events` 表（与 file_events 职责分离：
  原始层 vs 研判层，统一时间线 UI 合并展示）、采集层零去重（UI 折叠）、
  溢出→句柄保持+对账扫描→差分补齐→重启监视、关闭期间不声称完整记录、
  不认定 AI 来源、不引入 USN/进程/跨卷推断；
- **v7 迁移**：events 表零结构变更；watch_roots 存规范化路径 + 歧义拒绝；
- **完成判据**：对账差分台账 completed ∧ 自差分完成时刻通知流无新溢出；
- **测试**：TB1–TB10（授权 CRUD/生命周期/动作映射/零去重/溢出闭环/撤销精确范围）。

## 附：Git 提交链（main，未配置远程）

```
a6b43d1 docs(m3b-b1-4): 真机验收完成——迁移/端到端四类事件/台账/UI 一致性全部通过
087531e docs(m3b-b1-4): B1 实施交接报告——B1-0~B1-3 完成、门禁全绿、副本迁移验证通过
ead635c feat(m3b-b1-3): 历史追踪页启用——差分台账+文件事件真实展示
552b95b feat(m3b-b1-2): 接线——扫描范围/覆盖缺口收集+扫描后自动差分+卷登记规范化+B1 IPC
0a8e206 feat(m3b-b1-1): 差分引擎——四类事件/台账状态机/阻塞选对/歧义隔离
b25ace1 feat(m3b-b1-0): v6 迁移——差分台账+file_events 重建+scope/coverage 列+撤销域分治
749a789 docs(m3a): 真实环境启动验收报告
45eec97 feat(m3a): 真实库迁移验收例子 + 交接文档更新（M3-A 完成态）
```

**当前状态**：main 分支 · 工作区干净 · 未配置远程仓库 · 真实库 v6（应用自身迁移，
备份 `backups/pre-v6-final-20261106-010119\` 留存）· 应用已优雅退出。

---

*本报告全部数据来自真实执行的测试与验收；NOT RUN 项已明确标注（B2 全部、
真实库在线迁移前的自动化回归重跑）；无虚构、无未执行的"已通过"。*
