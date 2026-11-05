# FMap M3-B1 实施交接报告（M3_B_IMPLEMENTATION_HANDOFF · 单文件主报告）

> **更新时间**：2026-11-06 · **状态**：B1-0 ~ B1-3 完成并提交；B1-4 门禁全绿，
> 真实库副本迁移验证通过；**真实库迁移与真机启动验收等待用户授权**
> **设计基准**：docs/M3_B_DESIGN.md v6（已入库，随本轮提交）
> **提交链**：B1-0 `40be846→…`（各阶段独立提交，见 §8）

---

## 1. 当前完成阶段

| 阶段 | 状态 | 说明 |
|---|---|---|
| B1-0 数据库迁移 | ✅ 完成 | v6：scan_diff_runs 台账 + file_events 重建 + scope/coverage 列 + 撤销域分治 |
| B1-1 差分核心 | ✅ 完成 | pathnorm/scan_scope（core）+ diff_pass 引擎（database） |
| B1-2 流程接线 | ✅ 完成 | scanner 缺口收集 + scope/coverage 落库 + 扫描后自动差分 + 卷登记规范化 + IPC |
| B1-3 前端展示 | ✅ 完成 | 历史追踪页启用（台账+事件时间线）+ 语义标签 + vitest |
| B1-4 验收 | ✅ 门禁全绿 / ⏸ 真机部分 | 见 §7 迁移状态与 §9 授权申请 |

## 2. 修改的文件

**新增**：`tests/schema_v6.sql` · `crates/core/src/pathnorm.rs` ·
`crates/core/src/scan_scope.rs` · `crates/database/src/diff_pass.rs` ·
`apps/desktop/ui/src/pages/History.tsx` · `docs/M3_B_DESIGN.md`（v6 入库）· 本报告

**修改**：`crates/database/src/lib.rs`（SCHEMA_VERSION=6、v6 迁移块、upsert_volume
规范化、set_scan_scope/set_scan_coverage/diff_runs/history_events DAO、v6 迁移测试、
T50/T33/T54）· `crates/database/src/source.rs`（IDENTITY_DOMAIN_KINDS 域注册表、
revoke 身份域/观察域分治、撤销测试 v6 版）· `crates/core/src/scanner.rs`
（五类覆盖缺口收集 + MAX_COVERAGE_GAPS + 测试）· `crates/core/src/lib.rs`（模块注册）·
`apps/desktop/src-tauri/src/commands.rs`（scope/coverage 落库 + done 自动差分 +
get_diff_runs/get_history_events/run_diff_now）· `apps/desktop/src-tauri/src/lib.rs`
（命令注册）· `apps/desktop/ui/src/lib/{format,ipc}.ts` ·
`apps/desktop/ui/src/{App.tsx,pages/Placeholders.tsx}` ·
`apps/desktop/ui/src/__tests__/format.test.ts` ·
`tests/validate-schema.mjs`（v6）· `tests/check-readonly.mjs`（测试模块精确化）

## 3. 关键实现

1. **差分引擎**（diff_pass）：逻辑路径键=(volume_id, rel_key)（大写化规范化）；选对=
   done 代升序 + 台账连续前缀 + pending 重试语义（条件 UPSERT 仅覆盖 pending 行，
   completed 重放自动 no-op）；分类四事件（DISCOVERED「首次观察到」含 prev_gap 标记 /
   MODIFIED 元数据前后值 / MISSING 挂最后存在代节点 / REAPPEARED 带缺失起始代）；
   双事务（completed 台账+事件原子提交；pending 独立状态事务）；覆盖缺口/截断抑制
   MISSING；歧义四类全跳过；零事件对留 completed 台账。
2. **覆盖完整性**：scanner 五类跳过点收集（相对登记根规范化路径，上限 10,000 截断）；
   差分前置 scope 一致性校验（不一致/缺失 → 整对跳过 + 原因入台账）。
3. **域分治**：身份域事件按 kind 注册表删除；观察域（差分事件+台账）撤销时保留并
   解除实体引用（entity_id 去 CASCADE——实现中发现 v6 CHECK 顺序缺陷并修复：
   先删身份域再解引用，否则 entity-only 行违反表级 CHECK）。
4. **卷登记规范化**：normalize_root（大写+反斜杠+尾分隔符）；变体归并同一卷行；
   存量 root 字符串不动；多条等价冲突拒绝并要求人工处理。

## 4. 实际运行的测试及结果（全部真实执行）

| 套件 | 结果 |
|---|---|
| `cargo test --workspace` | ✅ **85/85**（core 25 · database 51 · platform 9），exit 0 |
| `cargo clippy --workspace` | ✅ 0 警告 |
| `cargo fmt --all -- --check` | ✅ |
| `npm test`（前端） | ✅ **11/11** |
| `npm run build`（含 tsc） | ✅ |
| `node tests/validate-schema.mjs` | ✅ 23 表（v1+v5+v6+补列）；CHECK/唯一索引/台账 UNIQUE 均验证拒绝 |
| `node tests/check-readonly.mjs` | ✅ |
| 真实库**副本** v5→v6 迁移（identity_real） | ✅ paths 508,032/508,032 · attributions 4,245/4,245（全 unknown）· tools 15/15 · volumes 3/3；node 交叉核对：台账结构/file_events 12 列/scans_running=0 |

**B1-1 diff_pass 9 项真机场景测试**（真实 tempdir 多代扫描驱动）：四类事件与零事件对、
台账连续前缀、缺失重现（missing_since=g3 口径）、绝不产 file_deleted、scope 缺失/
不一致整对跳过（原因可追溯）、覆盖缺口抑制 MISSING（missing_suppressed 可见）、
大小写重命名不拆分、歧义隔离（ß.txt+SS.txt 真实规范化碰撞：零事件/不合并/恢复后
不误记）、超限阻塞与两种人工恢复（事件严格按代序）、取消零写入、M3-A 语义红线复检
（差分不触碰 confirmations/candidates/attributions/source_*）。

## 5. 未通过的测试

**无**。所有测试均通过（未运行=NOT RUN：真实库在线迁移、真机启动验收、B2 全部——见 §9）。

## 6. 已知限制（按决策原则记录，非阻断）

1. **真实库 file_events 0 行 + 历史 done 代无 scan_scope** → 真实库升级 v6 后，需要
   **两次新扫描**才产生首个差分对（旧代缺 scope，差分按"整对跳过"保守处理——设计预期）。
2. rel_key 与 NTFS $UpCase 的极端 Unicode 理论差异（ß/SS 类）→ 已用**歧义隔离**兜底
   （碰撞时不合并、不产事件、台账计数可见）。
3. `run_diff_now` 为同步命令（B1 最小）；扫描进行中触发会等待写锁（busy_timeout 30s）。
4. 超限人工处置入口 = 本页"补跑差分"按钮 + 上限为编译期常量（DEFAULT_MAX_DIFF_EVENTS
   可参数化，UI 配置留后续）。
5. 历史追踪页事件上限 300 条（分页留 B2+）。
6. 双根输入仅 roots[0] 被扫描（M1 既有边界，未变）。
7. 只读门禁精确化：跳过 `#[cfg(test)]` 测试模块（测试仅操作 tempfile 自建 fixture；
   门禁意图=保护用户文件，脚本内注释说明）。

## 7. 数据库迁移状态

- **代码与测试**：v6 迁移完成，85/85 测试覆盖（v5→v6 含非空 file_events fixture、
  半迁移/脏残留收敛、约束验证）。
- **真实库副本**：已验证无损（§4 表格 + node 交叉核对；备份
  `backups/pre-v6-20261106-005346\`）。
- **真实库本体**：**仍为 v5，未动**（本轮预检只读确认）。

## 8. Git 状态

- 分支 main · HEAD `749a789` 起的 B1 提交链（B1-0/B1-1/B1-2/B1-3 四次提交）·
  工作区干净（本报告随最终提交入库）· **未配置远程，未 push**

## 9. 下一阶段任务与授权申请

**申请授权（涉及不可逆/用户数据操作，按约束停止待批）**：
1. **真实数据库 v5→v6 迁移**：方式=启动桌面应用由应用自身执行（同 M3-A 模式，
   已有副本验证 + pre-v6 备份）；或用户指定其他方式。
2. **真机启动验收**：启动应用 → 检查设置页/历史追踪页/真实扫描两轮 → 差分事件
   真实产生验证（包含一次"扫描-修改-再扫描"的端到端 MISSING/MODIFIED 场景）。
   该验收会连带完成真实库迁移（应用启动即迁移）。

**B2（RDCW，待 B1-4 真机验收通过后开始）**：watcher 模块 + watch_roots 授权 UI +
四层水位 gap 生命周期 + 溢出对账闭环 + TB 测试矩阵（设计 v6 §7/§11）。

---

*本报告全部结果来自真实执行；未运行的项标记为等待授权/NOT RUN，无虚构。*
