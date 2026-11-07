# FMap 最终发布报告（FMAP_FINAL_RELEASE_REPORT）

> **更新时间**：2026-11-07 · **阶段**：M3-C/D/E + M4 核心功能完成轮 · 基线 v0.1.0
> **仓库**：https://github.com/Yangxbao/FMap (Public · main)
> **本报告为唯一主报告**；历史版本在 Git 历史中。全部数据来自真实执行的测试与验证，NOT RUN 项如实标注。

---

## 1. 当前 Git 状态（真实）

| 项 | 值 |
|---|---|
| 分支 | main |
| HEAD | `0d26c78`（feat(m3-e+m4)） |
| 本轮新增提交 | `42765c2` M3-C · `afebf14` M3-D · `0d26c78` M3-E+M4 |
| 远程 | `https://github.com/Yangxbao/FMap.git` |
| 标签 | v0.1.0（既有，未新打标——按指令不为半成品功能打标签） |
| 工作区 | 干净（报告提交后） |

## 2. 本轮总览：停止发布打磨，全面开发核心功能

按总指令路线 **M3-C → M3-D → M3-E → M4** 连续推进，全部落地为可验证实现。
M5（统一完善）完成了其中可自动化验证的部分（全量测试/迁移验证/真机验证），
其余长周期项（长期运行稳定性）见 §8。

## 3. M3-C：AI 工具行为关联 ✅

**语义设计（可解释、可撤销、不误导）**：
- 关联 ≠ 来源：候选只表述"文件最后修改时间与工具活动观察存在**时间关联**"，
  绝不产出 AI_CREATED/AI_MODIFIED（来源行为唯一写入路径仍是 M3-A 用户确认）；
- **双上下文门槛**：路径上下文（文件位于工具活动工作区）**且**时间上下文
  （|文件 mtime − 信号时间| ≤ 窗口）同时满足才产候选；仅路径匹配是 M2 归属
  已覆盖语义，不是本引擎输出；
- 时间锚只用**文件系统 mtime**（信号自身 mtime / 目标文件 mtime）；
  差分运行时刻（file_events.ts）是"我们何时观察到"，绝不参与关联
  （避免同扫描假性互相关联）；
- 信号两类：特征文件（CLAUDE.md 类，窗口 ±60 分钟，置信 likely）/
  工具数据目录（.claude 类，窗口 ±24 小时，置信 possible）；
- 证据不足（mtime 缺失/超窗/不同工作区）→ 不产候选（无候选 = 诚实未知）；
- 四态区分：工具已识别（M2 归属）/ 时间关联候选（本引擎）/ 用户确认来源
  （M3-A）/ 来源未知（默认）。

**实现位置**：
| 组件 | 位置 |
|---|---|
| 纯逻辑引擎（13 测试） | `crates/core/src/correlation.rs` |
| schema v7（correlation_candidates 候选表） | `tests/schema_v7.sql` + `crates/database/src/lib.rs` v7 迁移 |
| 数据库 pass（信号发现/落库/审查状态机，9 测试） | `crates/database/src/correlation_pass.rs` |
| IPC | `apps/desktop/src-tauri/src/commands_m3c.rs`（3 命令） |
| 前端"行为关联"页（第 10 页） | `apps/desktop/ui/src/pages/Correlation.tsx` |

**审查能力**：确认关联 / 否认 / 忽略 / 撤回待审（可撤销）；重算时已审查行
永久保留、过期 pending 清理、新候选追加；scope 防御拒绝混挂历史数据。

## 4. M3-D：浏览器下载记录接入 ✅

**安全评估（先行，结论写入代码文档）**：
- Chrome/Edge `History` 是**当前用户可读的普通文件**（无 ACL 保护、无内容加密
  ——与 Login Data/Cookies 不同），不属于"受保护的数据库"；
- 运行中浏览器持有 SQLite 写锁但文件以 FILE_SHARE_READ 打开——**只读复制**
  是普通文件操作，不打开原库、不绕过任何进程锁/权限/安全机制；
- 最小信息：只 SELECT `downloads` / `downloads_url_chains` 的最小字段集，
  **绝不**触碰 urls/visits/Cookies/Login Data；referrer/tab_url 等浏览上下文
  一律不取不存（真机验证含诱饵表不触碰断言）。

**授权模型**：默认零授权（表无行 = 功能完全关闭）→ 逐浏览器 × 逐档案授权 →
可暂停/恢复 → 撤销（级联删除该源全部记录）/ 清除记录（保留授权）。

**实现位置**：
| 组件 | 位置 |
|---|---|
| 档案发现 + 只读副本（4 测试） | `crates/platform/src/browser_downloads.rs` |
| schema v8（browser_download_sources + browser_downloads） | `tests/schema_v8.sql` + v8 迁移 |
| 授权/导入/候选匹配（7 测试） | `crates/database/src/browser.rs` |
| IPC（7 命令） | `apps/desktop/src-tauri/src/commands_m3d.rs` |
| 设置页授权卡 | `apps/desktop/ui/src/components/BrowserDownloadsCard.tsx` |
| 真机验证示例 | `crates/database/examples/browser_real.rs` |

**下载 × 扫描 = 候选关联**（match_state），不认定来源。

## 5. M3-E：USN Journal、后台监控与跨平台 ✅（部分实现 + 完整设计）

**权限边界（决定性结论，真机实证）**：
| 能力 | 权限 | 决策 |
|---|---|---|
| 卷级 USN（ENUM_USN_DATA / READ_USN_JOURNAL） | 需管理员 | **不实现**（红线：不默认提权），完整设计留档待产品决策 |
| **单文件 USN（FSCTL_READ_FILE_USN_DATA）** | **普通句柄即可** | **已实现 + 真机验证** |

**已实现**（`crates/platform/src/journal.rs`，4 测试）：
- `read_file_usn_data()`：USN_RECORD_V2 解析（FileReferenceNumber /
  ParentFileReferenceNumber / 文件名；版本异常诚实拒绝）——真机 NTFS 实测
  成功，文件父引用 = 父目录自身引用；
- `probe_volume_usn()`：卷级访问探测（不提权不重试），如实报告需管理员；
- `journal_capability()`：一卷完整能力画像（fs_type 门控 NTFS/ReFS vs FAT/exFAT）。

**设计文档**（`docs/research/USN_JOURNAL_M3E_DESIGN.md`）：文件系统差异矩阵、
日志回卷/JournalID 变化/ERROR_JOURNAL_DELETED 边界、gap 语义（与 B2 同规范）、
安全降级、跨平台抽象现状（Windows 能力保持真实，其他平台 trait 位诚实
Unsupported——不做空壳）。

**后台监控不失控**（B2 已实现，本轮登记为验收项）：退出钩子取消全部会话、
无独立进程/服务/驱动。

## 6. M4：磁盘地图 + 文件浏览器 ✅

**全部基于已有扫描数据库，无演示数据**：
- **磁盘地图**（`apps/desktop/ui/src/pages/DiskMap.tsx`）：各卷容量条
  （登记时真实 GetDiskFreeSpaceExW）、最新扫描代状态、文件/目录计数、
  **覆盖缺口数**（未覆盖区域不当作完整）、**采集时间**（快照语义）、
  未扫描卷如实标注"未扫描"（绝不显示成空目录）、下钻入口；
- **文件浏览**（`apps/desktop/ui/src/pages/Files.tsx`）：面包屑导航、目录子项
  （目录优先/大小排序）、FTS 文件名搜索（各卷最新代）、详情抽屉
  （归属 + 观察事件 + mtime/ctime + 快照代标注）、复制路径、
  **打开所在目录**（explorer /select 只读定位）——绝不删除/移动/重命名文件；
- **实时监控**（`apps/desktop/ui/src/pages/Monitor.tsx`，替换占位）：授权根
  四层水位、监控缺口、最近原始通知（观察层）、USN 能力画像（真实探测）。

**实现位置**：查询层 `crates/database/src/browse.rs`（11 测试）·
IPC `commands_m4.rs`（8 命令）。

**修复的真实缺口**：`paths_fts` 全文索引**从未被任何代码写入**（schema 预留
"Rust 层批量维护"未落地）——本轮接线：scan_writer 收口时按卷重建；
真实库实测发现单事务写入 ~100 万行 FTS 会长时间停滞 → 改为 1 万行/块
独立事务；搜索改为内层先截断 FTS 匹配集（宽前缀响应上限保证）。

## 7. 数据库结构变化（v6 → v8）

| 版本 | 内容 | 真实库迁移验证 |
|---|---|---|
| v7 | `correlation_candidates`（M3-C 候选 + 审查状态机） | ✅ 真机副本 3,304,106 paths 零损失 |
| v8 | `browser_download_sources` + `browser_downloads`（M3-D） | ✅ 同上（同一迁移链） |
| 表总数 | 23 → **26** | validate-schema 26 表通过 |

迁移延续既有纪律：阶段事务、版本号与 DDL 同事务、全部 CREATE IF NOT EXISTS
非破坏、历史数据一行不动。

## 8. 测试结果（真实命令与输出）

| 套件 | 命令 | 结果 |
|---|---|---|
| Rust 全量 | `cargo test --workspace` | ✅ **134/0**（core 38 · database 76 · platform 20） |
| Rust 静态检查 | `cargo clippy --workspace` | ✅ 0 error / 1 既有 minor（m3b Ok+? 冗余） |
| 格式 | `cargo fmt --check` | ✅ |
| 前端测试 | `cd apps/desktop/ui && npm test` | ✅ **17/17** |
| 前端构建 | `npm run build`（含 tsc --noEmit） | ✅ |
| Schema 验证 | `node tests/validate-schema.mjs` | ✅ 26 表 |
| 只读门禁 | `node tests/check-readonly.mjs` | ✅（零例外：临时副本清理改由 tempfile 托管） |

**关键回归防护**：M3-C 引擎语义测试（双上下文缺一不可/时间锚纪律/证据不含
来源行为词）；M3-D 诱饵表不触碰断言；域注册完备测试继续覆盖新事件词表。

## 9. 真实环境验证（全部真实数据，无 mock 冒充）

| 验证项 | 方式 | 结果 |
|---|---|---|
| M3-D 真实 Edge 导入 | `cargo run -p tracenest-database --example browser_real` | ✅ Edge/Default 真实 History 副本解析 **110 条**（96 complete/14 cancelled），运行中浏览器不受影响，字段键集合验证最小信息 |
| M3-E 单文件 USN | platform 测试（真机 NTFS tempdir） | ✅ FileReferenceNumber/父引用/文件名交叉一致 |
| M3-E 卷级 USN 探测 | platform 测试 | ✅ 诚实报告需管理员，不提权 |
| M4 真实库迁移 + 浏览 | `cargo run -p tracenest-database --example browse_real -- <真实库>` | ✅ **v6→v8 迁移 3,304,106 paths 零损失**；卷概览（C:\ done 743K 文件 507 缺口 · E:\ done 316K · D:\ 未扫描如实为 None）；根目录浏览 22 项；FTS 重建（C 盘 ~95 万行分块）后 'tracenest' 命中 19 条 |

**NOT RUN（如实标注）**：
| 项 | 原因 |
|---|---|
| M3-C/M3-D/M4 的**应用内 UI 手工验收**（真机点按截图） | 需要 GUI 会话交互；代码路径已由 IPC + 单元测试 + 真实数据查询覆盖 |
| M3-E 卷级 USN 增量读取 | 红线内不实现（设计留档） |
| 长期运行稳定性（RDCW 连续多天） | 长周期项，需用户日常使用积累 |
| NSIS 安装包 | tauri build 需网络下载 NSIS 工具链（同上轮） |

## 10. 尚未完成的功能

| 功能 | 状态 | 说明 |
|---|---|---|
| M5 长周期项 | 部分完成 | 长期稳定性/大库性能回归需日常使用数据；本轮完成全量测试 + 真机迁移验证 |
| M3-E 卷级 USN 事件流 | 设计完成 | 红线内不激活（需管理员；设计见 §5 文档） |
| inotify/FSEvents 适配 | trait 位保留 | 诚实 Unsupported，不做空壳 |
| 项目资产页 / 报告中心 | 占位 | 非本轮范围 |
| NSIS 安装包 | NOT RUN | 网络限制（同上轮） |
| 历史页分页/高级搜索 | 待做 | B1 遗留低优先项 |

## 11. 当前已知风险

| # | 风险 | 缓解 |
|---|---|---|
| 1 | M3-C 候选量可能较大（数据目录信号窗口 24h，同工作区当日修改文件全部成候选） | 候选可否认/忽略/撤回；窗口为保守常量；证据完整可审查 |
| 2 | M3-D 匹配用 NOCASE（ASCII 折叠），与 rel_key 的 Unicode 大写化在极端字符有理论差异 | 匹配结果是候选非事实，可复查 |
| 3 | FTS 索引随扫描代重建（每卷最新代），旧代行被清理 | 幂等重建；索引缺失不影响差分/事件完整性 |
| 4 | 混挂历史数据（scan6 C+D+E）的归属被 scope 防御排除 → C 盘关联信号缺失 | C 盘重扫即恢复（既有已知项） |
| 5 | `reveal_in_explorer` 使用 explorer /select 进程派生 | 只读定位动作，无文件修改参数 |
| 6 | 前端 UI 手工验收未做 | IPC/数据层已测；建议用户启动应用过一遍新页面（§12） |

## 12. 需要用户手动操作的事项

1. **应用内验收**：`cargo tauri dev` 启动 → 新增页面走查：
   行为关联（重新计算 → 审查动作）· 设置页浏览器下载卡（授权 Edge → 导入）·
   磁盘地图 / 文件浏览 / 实时监控；
2. **真实库升级**：下次启动应用时自动执行 v6→v8 迁移（真机副本已验证零损失）；
   FTS 索引在下一次扫描收口时自动重建；
3. （可选）在有网络的环境构建 NSIS 安装包；
4. （可选）为 M3-C/D/E + M4 完成态打版本标签（如 v0.2.0）——由用户决定时机。

## 13. 下一阶段最合理的入口

1. **应用内 UI 手工验收**（本轮唯一未覆盖的验证层）；
2. **用户真实使用 M3-C**：运行归属分析 + 差分后点"重新计算关联"，
   用真实候选量评估 24h 窗口是否需要收窄；
3. **M3-D Chrome 扩展路线评估**（可选）：当前文件复制方案已覆盖 Chrome/Edge；
   若需要"零浏览器文件访问"的方案，再评估 DownloadManager 扩展；
4. **M5 长周期项**：随日常使用收集性能数据（大库差分耗时、UI 大目录渲染）；
5. **项目资产页**（占位中）：可复用 browse.rs 的聚合查询。

---

## 附录：本轮安全红线执行核对

| 红线 | 执行 |
|---|---|
| 不移动/删除/重命名/修改用户文件 | ✅ 只读门禁通过（临时副本清理改 tempfile 托管保持零例外） |
| 默认只读 / 不读内容 / 不算哈希 | ✅ 无任何内容读取代码路径 |
| 不读 AI 会话/JSONL | ✅ |
| 不上传数据 / 零网络出站 | ✅ 无任何网络代码 |
| 敏感功能默认关闭、逐项授权 | ✅ M3-D 默认零授权（表无行 = 关闭）；M3-C 为派生数据无需新授权 |
| 用户可暂停/撤销/清理 | ✅ 两侧授权卡均具备 |
| 不关 Defender / 不绕权限 / 不默认管理员 | ✅ USN 卷级读取因需管理员而**不实现**（设计留档） |
| 不安装系统组件 | ✅ 仅新增 crates.io 依赖（tempfile） |
| UNKNOWN 永久合法 / 不编造来源 | ✅ 引擎与候选语义测试强制 |
| 不伪造测试结果 | ✅ 全部 NOT RUN 如实标注 |

*本报告全部数据来自真实执行的测试与验证。*
