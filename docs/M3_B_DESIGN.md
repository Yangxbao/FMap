# M3-B 基础事件追踪 · 设计文档（M3_B_DESIGN）

> **版本**：**v6**（第五轮审核定点修订）· **日期**：2026-11-06
> **状态**：仅设计——**不含任何业务代码修改**；v6/v7 迁移未执行；B1/B2 实现未启动
> **审核链**：v1 → 一审 → v2 → 二审（RDCW 纳入）→ v3 → 三审 → v4 → 三审复审 → v5 → 四审（五项）→ **本 v6**
> **事实基准**：全部代码事实已在当前 HEAD `749a789a2be7f2b9686b1b8d95e2cb15f48aa508`
> 重新核对（附 A）；【已验证事实】/【设计建议】/【待用户决定】/【待实现】四态严格区分
> **HEAD 关系**：45eec97（M3-A 验收报告所记）为当前 HEAD 直系祖先（749a789 为其子提交）——已验证，无矛盾
> **用户既定决定（不可再议）**：RDCW 纳入 M3-B（可选目录监控，B1 验收通过后实施）；B1 先于 B2

---

## 0. v6 变更摘要（相对 v5）

| # | 变更 | 动因（五审问题号） |
|---|---|---|
| C1 | **双事务分离**：正常差分事务（事件+completed 台账原子提交）与**超限状态事务**（独立小事务写 pending_over_limit）彻底分开；超限检测发生在分类阶段（内存），部分事件结构性不可能；独立状态事务失败 → 不丢超限事实（每次触发重算），下次自然重建 pending | 问题 1（v5 中 pending 与差分同事务会连带回滚） |
| C2 | **台账状态机统一**：completed/skipped_scope_mismatch/skipped_over_limit=已处理（允许后续），pending_over_limit=阻塞；**台账行必须构成从基线起的连续前缀**（不变式）；跳过后下一对=跳过行的 to_scan_id 起始；pending 行可人工改判，**completed/skipped 历史行禁止重开**；三代推演入档 | 问题 2（v5 SQL 只算 completed 与 §1.3/§8 冲突） |
| C3 | **溢出完成判据可验证化**：四层水位模型（接收→持久化→研判→gap 关闭）；引入**溢出代次 g**（会话内单调递增）与**持久化水位**（persist_seq=events 最大 rowid）；gap 关闭 ⇔ 研判完成 + 当前代次处理完毕 + 持久化水位=接收水位；关闭判定与新溢出在同一互斥区内先判后增（竞态消解）；**保证边界如实声明**（溢出瞬间通知已丢不可恢复——不宣称绝对无遗漏） | 问题 3 |
| C4 | **evidence_id 兼容性核实定稿**：【已验证事实】现 DDL 中 evidence_id 本就可空（schema_v5.sql:63 无 NOT NULL）→ 差分事件 NULL 引用无需改列；v6 重建**新增 `diff_run_id INTEGER REFERENCES scan_diff_runs(id)` 可空列**（强关联替代 detail 内弱引用），台账先行建表 | 问题 4 |
| C5 | **volume.root 存量兼容规则**：登记时规范化 → 查等价存量 → 唯一匹配**复用原 volume_id**（历史数据不动）；**多条存量规范化冲突 → 拒绝登记并提示人工处理，绝不静默新增第三条**；Unicode 碰撞与 rel_key 同策略（大写化键 + 冲突人工） | 问题 5（补核对发现 upsert_volume 无规范化：lib.rs:307-315） |
| C6 | **歧义生命周期定稿**：歧义是**判定时性质，不持久化**（每次差分重检测，零成本）；歧义键在差分分类中**四类事件全跳过**；历史事件保留不撤销；解除=某次判定时该键恢复单值，自动恢复正常追踪（中间期为诚实缺口）；台账以 `ambiguous_count` 轻量计数（不独立建表） | 问题 6（S1→S4 ß/SS 推演） |
| C7 | 测试扩至 **T50–T55**（volume 存量兼容/歧义生命周期/超限状态事务故障注入/台账连续前缀不变式/evidence NULL+diff_run_id FK/水位四层与竞态） | 四审测试要求 |

v1–v5 已定稿设计（原子对事务、事件推导缺失集、S1→S6、域注册表、CHECK、零采集去重、
授权规范化/嵌套、USN 留 M3-E、B1/B2 拆分、阻塞策略、台账化、歧义隔离、溢出句柄保持）
**全部保留，不退回讨论**。

---

## 1. 双事务模型与超限状态（C1，三审问题 1 终稿）

### 1.1 两类事务的严格区分【设计建议，待确认 D2】

| | 正常差分事务 | 超限状态事务 |
|---|---|---|
| 触发 | 分类完成且事件数 ≤ 上限 | 分类完成且事件数 **> 上限** |
| 内容 | INSERT 事件（逐条）+ INSERT 台账行（status='completed', events_count, ambiguous_count） | UPSERT 台账行（status='pending_over_limit'，scope_json/超限元数据） |
| 事务边界 | BEGIN IMMEDIATE → 全部写入 → COMMIT | BEGIN IMMEDIATE → 单行写入 → COMMIT |
| 失败行为 | 全回滚（事件+台账一并消失，该对回到"未处理"） | 单行回滚（pending 状态丢失，见下） |

**完整流程（问题 1 逐条回答）**：

```
run_scan_diff(对 (S_prev, S_curr)):
  1. 分类（纯内存）：产出事件清单 + 歧义排除 + 缺失集预载 —— 无任何写入
  2. if 清单 ≤ 上限:
       BEGIN IMMEDIATE
         INSERT 事件 × N（Rust 侧逐条累计计数，防御性复查上限，
                          超限 → ROLLBACK → 转 3）
         INSERT 台账行(status='completed', events_count=N, ambiguous_count=..)
       COMMIT                      # 事件与 completed 台账原子提交 ✅
  3. if 清单 > 上限（或事务内防御超限）:
       —— 不存在"部分事件"：清单在内存中，差分事务未开启或已整体回滚 ✅
       独立超限状态事务:
         BEGIN IMMEDIATE
           UPSERT 台账行(status='pending_over_limit', scope_json, 元数据)
         COMMIT                      # 与差分事务完全分离，不受其回滚影响 ✅
       if 本事务失败（BUSY 超时等）:
         —— pending 行丢失，但超限事实不丢：分类每次触发重算（事件量是
            客观事实），下次触发/扫描后再次超限 → 再次尝试写 pending（自然收敛）
         向调用方返回明确错误："该对超限且状态记录失败（原因），将自动重试；
            差分研判已暂停"——UI 提示常驻（§13 U22）
```

- **不产生部分事件**：分类在内存、写入前两道上限检查（v4 定稿）——结构保证；
- **不丢失超限状态**：pending 行是**持久化提醒**而非唯一事实源——事实源是"该对
  事件量超限"这一每次可重算的客观属性；即使 pending 行写入失败，下次触发重建；
- **故障注入测试 T52**：注入"pending 状态事务失败"→ 断言：差分对无事件、游标
  （台账派生）不动、下次触发重新检测超限并成功重建 pending 行。

### 1.2 上限值与阻塞（维持 v5：50,000 待确认 D2；阻塞定稿）

---

## 2. 台账状态机统一（C2，三审问题 2 终稿）

### 2.1 状态语义表【设计建议，待确认 D1/D4】

| status | 含义 | 已处理？ | 对后续对的影响 | 可人工改判？ |
|---|---|---|---|---|
| `completed` | 已研判（含零事件） | ✅ | 允许后续对继续 | **否**（append-only，重开会重复事件） |
| `skipped_scope_mismatch` | 人工/自动确认放弃（范围不一致） | ✅ | 允许后续对继续 | 否 |
| `skipped_over_limit` | 人工确认放弃（超限放弃分支） | ✅ | 允许后续对继续 | 否 |
| `pending_over_limit` | 超限待人工 | ❌ **阻塞** | **该对及之后全部停止** | **是**（唯一可改判状态）：→ skipped_over_limit（放弃）或 删除行+提高上限重跑（pending 行从未伴随事件提交，删除无历史损失） |

### 2.2 队列/台账/历史顺序的一致性规则

- **派生游标（v5 §1.5 SQL 修正）**：不是简单 `MAX WHERE completed`——正确规则：
  **从基线起的连续已处理前缀的末端**。因阻塞保证"pending 之后不会有任何台账行"，
  连续前缀等价于：
  ```sql
  SELECT MAX(to_scan_id) FROM scan_diff_runs
   WHERE volume_id=?1 AND status <> 'pending_over_limit'
  ```
  在"无洞"不变式下与连续前缀语义等价（**不变式（T53）**：台账行集合必须构成
  基线起的连续前缀——存在 gᵢ 有行而 gᵢ₋₁ 无行且 (gᵢ₋₁,gᵢ) 非基线对 → 违例，
  校验函数在每次选对前断言）；
- **跳过后下一对**：从跳过行的 `to_scan_id` 起始（台账 to_scan_id 链条连续：
  前一对 to = 后一对 from）；
- **提高上限恢复**：pending 行删除（或改判 skipped）→ 该对按正常流程重跑
  （重新分类——上限已调高则 completed）→ 后续对依序补跑；
- **不允许重开 completed/skipped 历史行**（append-only + 事件已入库；历史重开 =
  重复事件/不变式破坏）；对历史区间的"补充研判"需求一律指向后续对（未来扫描
  产生新代后自然覆盖最新状态）；
- **缺失集顺序正确性**：阻塞保证事件插入严格按代序（v5 论证维持）；
- **三代推演**：
  ```
  例 A（阻塞→放弃恢复）：
    (S1,S2) 分类超限 → 独立事务写 pending_over_limit → 队列停止
    （S2,S3)/(S3,S4) 即使扫描完成也不处理，零事件、无台账行）
    人工确认放弃 → 该行改 skipped_over_limit
    → (S2,S3) 执行 completed → (S3,S4) completed
    缺失集推导按 id 序=代序 ✅
  例 B（阻塞→提高上限恢复）：
    pending 行删除 → (S1,S2) 重分类（上限内）→ completed(events_count=N)
    → (S2,S3) → (S3,S4) 依序 —— 与"从未超限的一次性执行"结果逐字段一致（T45 扩展）
  例 C（正常无阻塞）：
    各对 completed，MAX(to_scan_id)=最新研判代 —— 与 v5 一致
  ```

---

## 3. RDCW 溢出完成判据的可验证化（C3，三审问题 3 终稿）

### 3.1 四层水位模型（明确区分，不得混同）【设计建议，待确认 D10】

| 层 | 含义 | 载体 | 可观测信号 |
|---|---|---|---|
| L1 通知已**接收** | RDCW 调用返回的缓冲已解析 | watcher 内存队列（recv_seq 单调计数） | 内存计数器（B2 实现提供查询） |
| L2 通知已**持久化** | 已写入 events 表 | events 行（**persist_seq = 该根 events 最大 rowid**） | SQL 可查 |
| L3 差分语义已**研判** | file_events 语义事件产生 | scan_diff_runs（status='completed'）+ file_events | SQL 可查 |
| L4 watch_gap 已**确认关闭** | 缺口对账完成且无新缺口 | gap 元数据（meta：watch_gap_<键>，状态字段） | SQL 可查 |

**接收 ≠ 持久化 ≠ 研判 ≠ gap 关闭**——每层推进独立可见，UI 分别展示
（"实时通知 N 条已记录 / 已落盘 M / 研判待下次差分 / 缺口已对账"）。

### 3.2 溢出代次与完成判据【设计建议】

- **溢出代次 g**：每监控根会话内单调递增；每次溢出 g += 1；gap 元数据记录其
  g 值与区间起点；**当前代次 g_cur** 由 watcher 内存维护；
- **gap 关闭 ⇔ 三条全部成立（同一次互斥判定内评估）**：
  1. **研判完成**：该 gap 的对账差分台账行 status='completed'
     （L3 ≥ L2 的研判水位——覆盖到扫描完成时刻）；
  2. **代次处理完毕**：g_cur 所在代次的对账流程已走完（无 in-flight 溢出处理）；
  3. **持久化水位排空**：persist_seq == recv_seq（L1 全部落盘，无积压）。
- **互斥区竞态消解（关闭 vs 新溢出同时）**：溢出计数递增与 gap 关闭判定在
  watcher 消费线程的**同一互斥区**内串行——顺序只有两种：
  - 先判定后溢出：判定时 g 未变 → 若条件满足则关闭，随后新溢出开启**新 gap**
    （新代次）——两段历史各自完整；
  - 先溢出后判定：判定看到 g 已递增 → 条件 2 不满足 → 不关闭，进入新一轮闭环；
  **不存在"关闭了但还有未处理溢出"的窗口**。
- **时序推演**：
  ```
  T0 溢出(g=1) → 通知继续消费 → T1 触发对账扫描（句柄保持）→ T2 扫描 done
  → T3 差分 completed（研判覆盖 [基线, T2]）→ T4 persist_seq==recv_seq
  且 g 仍 =1 → gap(g=1) 关闭 ✅
  变体：T3.5 再次溢出(g=2) → T4 判定时 g=2 ≠ gap.g=1 的处理代次已推进？
  —— 处理规则：g 递增时旧 gap 的对账流程继续走完（差分仍有效），旧 gap 在
  其对账完成且此后无新溢出时关闭；新 gap(g=2) 独立记录（区间=溢出时刻起）；
  UI 显示两个 gap 或合并为最新未关闭 gap（实现细节 B2 定，语义：任一未关闭
  gap 存在 ⇒ 界面显示"存在未对账缺口"）
  ```

### 3.3 保证边界（诚实声明，三审要求）【不可退化】

- **RDCW 溢出瞬间之前的通知已被 OS 缓冲覆写，永久丢失，不可恢复**——任何方案
  无法改变；我们保证的是：**每次丢失段都会产生一个如实呈现的未关闭 gap**，且
  丢失段的可恢复部分（文件仍存在者的当前状态）由对账差分的快照对比覆盖
  （"消失"类丢失段不可恢复——file_missing 语义只对快照对成立）；
- **不宣称绝对无遗漏**：本设计的承诺是"丢失必留痕（gap 未关闭）、可恢复部分
  必研判（差分）、分层状态可见（四层水位）"；
- scan 完成时的通知水位：对账差分完成时刻的 persist_seq 记入 gap 元数据，
  作为 L2/L3 交汇审计点。

---

## 4. file_events.evidence_id 兼容性（C4，三审问题 4 定稿）

【已验证事实】当前 `tests/schema_v5.sql:58-67` 完整 DDL：

```sql
CREATE TABLE IF NOT EXISTS file_events (
  id INTEGER PRIMARY KEY,
  entity_id INTEGER NOT NULL REFERENCES file_entities(id) ON DELETE CASCADE,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,
  evidence_id INTEGER REFERENCES source_evidence(id),   -- 可空（无 NOT NULL）
  scan_id INTEGER,
  detail TEXT
);
```

- **evidence_id 可空**（无 NOT NULL）→ 差分事件 NULL 引用**当前结构即合法**，
  v6 无需为可空性改列；
- 外键：可空外键——NULL 不触发约束 ✅；
- 旧数据迁移：v6 重建搬迁时 evidence_id 原样保留（真实库 file_events 0 行，
  搬运量为零）；
- **新差分事件插入**：evidence_id=NULL 合法 ✅；
- **detail.diff_run 弱引用升级（v6 定稿）**：v6 重建时新增
  **`diff_run_id INTEGER REFERENCES scan_diff_runs(id)`（可空）** + 索引
  `idx_events_run (diff_run_id)`——台账与事件的关联从 JSON 弱引用升级为数据库级
  强外键（可查询、可校验、可级联理解）；建表顺序：v6 事务内 **scan_diff_runs
  先建**，file_events 重建后建；
- v6 file_events 最终列清单：id / entity_id(可空) / path_id(可空，新) /
  volume_id(NOT NULL，新) / rel_path(新) / rel_key(NOT NULL，新) / ts / kind /
  evidence_id(可空，原样) / **diff_run_id(可空，新)** / scan_id / detail
  + 表级 CHECK(entity_id 或 path_id 非空) —— 汇总见 §9。

---

## 5. volume.root 存量规范化兼容（C5，三审问题 5 定稿）【设计建议，并入 D1】

【已验证事实】`upsert_volume` 对 root **无任何规范化**（lib.rs:307-315 原样
INSERT/UPSERT）——`e:\project` 与 `E:\PROJECT` 会成为两条 volume。

**登记规则（v6 实现）**：

```
register_volume(新 root R):
  key = normalize(R)          # 大写化 + '/' 分隔 + 尾分隔符统一（与 rel_key 同函数）
  存量匹配 = volumes 表中 normalize(root) == key 的行（小表全扫）
  ├─ 0 行    → INSERT（root 存 normalize(R) 规范化形式）
  ├─ 恰 1 行 → **复用该 volume_id**（按 id UPDATE 容量等；root 原样不动
  │            —— 历史数据不修改）✅ 场景"存量 e:\project + 新登记 E:\PROJECT"
  │            → 复用原行，无第三条、无历史改动
  └─ ≥ 2 行  → **拒绝登记**（返回结构化错误），提示人工处理：
               "存在 N 条规范化等价的卷登记（列出 id/root），请人工合并后重试"
               —— 不自动合并、不静默新增第三条 ✅
               人工流程：诊断（列出冲突组与各自的历史数据量）→ 用户决定保留哪条
               （人工操作，工具不代做）→ 冲突解除后重试
```

- **历史扫描数据不动**：volume_id 不变 ⇒ paths/attributions/file_entities 全保留；
- **Unicode 碰撞兼容**：normalize 与 rel_key 同函数（大写化）——两条存量 root
  规范化后相同即"≥2 行"分支 → 人工处理，与 rel_key 歧义策略（§6）**同一防错哲学**：
  检测冲突、拒绝自动合并、交人工；
- scope 比较：基于规范化后的 scope JSON（roots 经同一 normalize）；
- 不同挂载点/不同登记根独立身份——维持 v4 §1.1。

---

## 6. rel_key 歧义生命周期（C6，三审问题 6 定稿）【设计建议，D9】

### 6.1 S1→S4 推演（ß.txt / SS.txt 同 rel_key=K）

| 差分对 | 判定时 K 的原样值集合 | 结果 |
|---|---|---|
| 基线（S1） | {ß.txt}（单值） | 基线无事件；K 正常入内存映射 |
| (S1,S2) | S2 中 K = {ß.txt, SS.txt} → **歧义首次成立** | **四类事件全跳过**；台账 ambiguous_count += 1；两文件均无 file_events 事件 |
| (S2,S3) | S3 中 K = {ß.txt}（单值）；S2 侧 K 歧义 | **仍然跳过**——判定需要双代同键可比，任一侧歧义即跳过（不冒合并风险） |
| (S3,S4) | 双代 K 均单值 {ß.txt} | **歧义解除，恢复正常**：两代均有 → 无事件（size/mtime 未变）；若值变 → MODIFIED 正常产生 |

**逐条回答**：
- **歧义何时成立**：判定时（该次差分分类内存中）某键对应 ≥2 个原样 rel_path——
  首次成立在 (S1,S2)；
- **影响哪些扫描代**：判定所及的差分对（双侧任一代含歧义即跳过）——影响 (S1,S2)
  与 (S2,S3)，(S3,S4) 恢复；
- **是否永久**：**否**——歧义是判定时性质，不持久化；
- **如何解除**：磁盘上该键恢复为单一原样名（SS.txt 消失或改名出键）→ 下一差分
  自动恢复正常；
- **解除后是否重新追踪**：是——从解除的差分对起恢复正常分类；**中间歧义期是
  诚实缺口**（无事件、台账 ambiguous_count 有计数、UI 可见"若干路径因名称歧义
  暂缓追踪"）；
- **历史事件保留**：歧义前的事件（如基线后 ß.txt 单值期的 DISCOVERED）append-only
  保留，不撤销；
- **缺失集防合并**：**两道检测**——(a) SQL 层：缺失集查询对每键
  `HAVING COUNT(DISTINCT rel_path)=1`（历史事件聚合的歧义排除）；(b) 内存层：
  差分分类时双代集合的同键多值检测——覆盖"事件行恰好只记录过其中一个文件"的
  情形（事件历史无碰撞但当代有碰撞）；
- **是否独立持久化歧义记录**：**不需要**——重检测零成本且天然反映当前事实；
  持久化反而引入"歧义状态与磁盘事实漂移"的新维护负担。轻量痕迹 = 台账
  `ambiguous_count` 列（每对计数，不建独立表）——"不为极端情况过度设计"✅。

### 6.2 规则汇总（差分分类伪代码更新）

```
对每个候选逻辑路径键 K：
  if 双代任一侧 K 对应多个原样 rel_path（或缺失集 K 被歧义排除标记）:
      跳过（不产任何 kind 事件）；ambiguous_count += 1
  else: 按 §3.4 正常分类
```

---

## 7. RDCW（维持 v5 §7 框架；溢出闭环按 §3 修订；其余——授权规范化/嵌套/
零采集去重/职责边界/USN 边界/生命周期——维持 v4/v5）

## 8. 差分对选择算法（维持 v5 §8，阻塞语义 + §2.2 连续前缀不变式）

## 9. 数据库变更提案（v6 = B1；v7 = B2）【待确认 D1–D6、D9、D10】

| # | 变更 | v6 相对 v5 |
|---|---|---|
| V6-1 | file_events 重建：entity_id/path_id 可空 + **CHECK(entity_id OR path_id 非空)** + volume_id NOT NULL + rel_path/rel_key + **diff_run_id 可空 FK→scan_diff_runs** | + diff_run_id（C4）；evidence_id 可空性经核实**无需改动** |
| V6-2 | 索引：唯一三元组 / vol_relkey / path_ts / **events_run(diff_run_id)** | + 索引 |
| V6-3 | scans.coverage_gaps（相对坐标系+kind 分类）+ scans.scan_scope | 维持 |
| V6-4 | **scan_diff_runs 台账**（v5 §4.1 DDL + **ambiguous_count 列**） | + ambiguous_count |
| V6-5 | revoke 域删除 + 孤儿清理 | 维持（待实现） |
| V6-6 | paths.deleted_at 注释修正 | 维持 |
| V6-7 | upsert_volume 登记规范化（存量兼容三段规则 §5） | 维持+兼容规则细化 |
| v7 | events 零结构变更；watch_roots 规范化存储 + 歧义拒绝 | 维持 |

（~~meta 游标 / meta 跳过键 / 零事件 source_evidence 证据~~ 自 v5 起废止，维持。）

## 10. 前端展示（维持 v4/v5：首次观察到文案、×N 折叠、嵌套覆盖标注、
四层水位分层展示 §3.1、歧义暂缓提示 §6.1）

## 11. 测试矩阵（v6：T1–T55 + TB）

v5 的 T1–T49/TB 保留；新增/修订：

| # | 用例 | 断言 |
|---|---|---|
| T50（新） | **volume 存量兼容三段**：a) 存量 `e:\project` + 新登记 `E:\PROJECT` → 复用原 volume_id（历史 paths/attributions 不动）；b) 全新路径 → 新建规范化行；c) 存量两条规范化等价行 → **拒绝登记**（结构化错误，不新增第三条），人工处理流程提示可见 | 三段逐项 |
| T51（新） | **歧义生命周期（三审 S1→S4 场景）** | (S1,S2)/(S2,S3) 零差分事件 + 台账 ambiguous_count≥1；(S3,S4) 恢复正常分类；ß.txt 的历史事件（如有）不被撤销；缺失集两道歧义检测均生效 |
| T52（新） | **超限状态事务故障注入** | pending 写入失败 → 差分对零事件、派生游标不动、错误上报；下次触发重新检测超限并成功重建 pending 行 |
| T53（新） | **台账连续前缀不变式** | 正常序列通过；人工构造"洞"（gᵢ 有行而 gᵢ₋₁ 无行）→ 校验函数报违例 |
| T54（新） | **evidence_id NULL + diff_run_id FK** | 差分事件插入：evidence_id=NULL 合法；diff_run_id 指向台账行；插入不存在的 diff_run_id → FK 拒绝 |
| T55（新） | **四层水位与关闭竞态** | 正常闭环（L1→L4 逐层断言）；关闭判定与新溢出竞态两序均不产生"已关闭但有未处理溢出"；scan 期间修改 → L1/L2 有记录、L3 待下轮（§3.2） |
| T45（扩展） | 超限阻塞全场景 | + 状态转换表（§2.1）逐格断言（pending 改判/删除语义） |

## 12. 分阶段开发计划（维持 B1/B2 拆分）

B1-0（v6：含 scan_diff_runs 先建 + diff_run_id 列 + volume 规范化）→ B1-1（diff_pass：
双事务模型 + 台账状态机 + 歧义生命周期 + T1–T55）→ B1-2（接线 + scanner 收集 +
scope 参数化 + revoke 精确化）→ B1-3（前端）→ B1-4（验收）；B2-0..B2-3（v7 + watcher
四层水位 + UI + 验收）。

## 13. 风险与未决问题（v6）

| # | 问题 | v6 状态 |
|---|---|---|
| U1–U20 | 维持 v5（U10 并入 D2；U12 台账化定稿；U16 关闭） | 维持 |
| U21–U23 | 维持 v5（台账 UNIQUE 幂等/阻塞可用性代价/gap 长期未关） | 维持 |
| **U24（新）** | 台账 ambiguous_count 只计数不记路径——人工排查歧义需重跑分类（可接受：歧义极罕见且重检测零成本） | 已声明取舍（D9 一并追认） |
| **U25（新）** | 四层水位中 L1（recv_seq）为进程内存态——应用崩溃后 L1 清零，未持久化通知丢失 | 诚实边界：L1 仅用于运行中排空判定；崩溃场景由 gap 未关闭 + 下轮差分兜底（与"不宣称绝对无遗漏"一致） |

## 14. 编码准入评估（v6 重评）

**已解决并定稿（v1–v6 累计）**：v5 全部 + 双事务模型（pending 独立持久化与故障
恢复）、台账状态机（状态转换表/连续前缀不变式/禁重开）、溢出四层水位与关闭竞态
消解、evidence_id 兼容核实（无需改列）+ diff_run_id 强关联、volume 存量兼容三段
规则、歧义生命周期（不持久化+四类全跳过）。

**仍存在的技术问题**：无已知**未闭环**技术矛盾——六项问题均给出自洽方案；剩余
为**实现期工作**（scanner 收集、scope 参数化、diff_pass/watcher、revoke 域删除、
volume 规范化登记——均标注待实现）。

**待用户决定**：D1（v6 条目 v6 版：**+diff_run_id 列、台账+ambiguous_count**）、
D2（上限 50,000+阻塞+双事务）、D3（revoke 域删除）、D4（**台账方案含 diff_run_id**
追认）、D5（coverage_gaps 改名/阈值/相对坐标系）、D6（deleted_at 追认）、D8
（DISCOVERED 产生+标记）、**D9（歧义隔离+生命周期不持久化）**、**D10（溢出四层
水位判据）**、D7-a/b（B2）。D7-c 已关闭。

**B1 编码准入**：**未完全达到**——设计已自洽（无未闭环技术矛盾），但 D1–D6、
D8、D9 的最终裁定（尤其台账/diff_run_id/歧义生命周期三处 v6 演进）须本轮审核
追认后方可开工 B1-0。
**B2 编码准入**：**未达到**——前置 = B1-4 验收通过 + D7-a/b + D10 追认。

---

## 附 A：代码事实索引（HEAD `749a789a2be7f2b9686b1b8d95e2cb15f48aa508` 本轮复核）

| 事实 | 位置 | 状态 |
|---|---|---|
| **file_events.evidence_id 可空**（无 NOT NULL；可空 FK） | tests/schema_v5.sql:63 | 已验证 |
| upsert_volume 对 root 无规范化 | crates/database/src/lib.rs:307-315 | 已验证 |
| revoke 现状全表 DELETE（域删除未实现） | crates/database/src/source.rs:191、:194 | 已验证 |
| scan_start 硬编码 ScanOptions::default() | apps/desktop/src-tauri/src/commands.rs:215 | 已验证 |
| 台账/歧义/diff_run/游标 | — | **待实现**（设计态，无代码） |
| RDCW/watcher | — | **未实现**（platform/lib.rs:47 仅能力注释） |
| coverage_gaps/scan_scope 收集能力 | — | **待实现**（scanner.rs:78/169 仅计数） |
| volumes.root UNIQUE | tests/schema.sql:19 | 已验证 |
| 多根仅 roots[0] 扫描 | commands.rs:191、:227 | 已验证 |
| rel 生成 | scanner.rs:261-263 | 已验证 |
| paths 按代追加 | scan_writer.rs:79-104 | 已验证 |
| file_events/source_evidence DDL | tests/schema_v5.sql:58-67 / :46-53 | 已验证 |
| events / watch_roots DDL | tests/schema.sql:124-134 / :137-142 | 已验证 |
| 确认/REUSE 事件写入 | source.rs:344；identity_pass.rs:313 | 已验证 |
| 事务全 DEFERRED / busy_timeout 30s | scan_writer.rs:79 等；lib.rs:85 | 已验证 |
| deleted_at 影响历史归属统计 | attribution_pass.rs:101、:438 | 已验证 |
| 退出取消（RunEvent::Exit） | src-tauri/lib.rs:164-170 | 已验证 |

## 附 B：变更日志

- v1 初稿；v2 一审；v3 二审（RDCW 纳入）；v4 三审定点；v5 四审定点（歧义隔离/
  相对坐标系/阻塞/台账/溢出句柄保持/volume 规范化）。
- **v6（本文档）**：五审定点修订——C1 双事务分离（pending 独立持久化+故障重建）；
  C2 台账状态机统一（状态语义表/连续前缀不变式/禁重开/三代推演）；C3 溢出四层
  水位与溢出代次（完成判据可验证化/竞态消解/保证边界声明）；C4 evidence_id 兼容
  核实（可空无需改列）+ diff_run_id 强关联列；C5 volume 存量兼容三段规则（复用/
  拒绝/人工）；C6 歧义生命周期定稿（判定时性质不持久化/四类全跳过/两道检测/
  ambiguous_count）；测试 T50–T55；§14 重评。

*本设计不含任何已执行的代码修改；v6/v7 迁移未执行；实现未启动。等待 ChatGPT 与用户审核。*
