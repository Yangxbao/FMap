# FMap M2 最终交接报告（M2_FINAL_HANDOFF · 单文件交接制）

> 日期：2026-11-04 · 执行：主会话 AI（零子代理）· 分支 main · HEAD 见 §16
> 用途：**只上传本文件即可让 ChatGPT 审查 M2 成果并规划 M3。**

---

## 1. M2 完成情况总览

| 任务书目标 | 状态 | 核心产出 |
|---|---|---|
| 任务一：AI 工具规则库 | ✅ 完成 | `rules/builtin-rules-v1.json`（15 工具/29 规则）+ 幂等加载器 + 启用/禁用 + 版本管理 |
| 任务二：信号文件检测 | ✅ 完成 | 引擎级信号匹配（CLAUDE.md/AGENTS.md/.cursorrules/GEMINI.md/.roo/.clinerules 等）；仅文件名，零正文读取 |
| 任务三：归属判断引擎 | ✅ 完成 | 四级证据（Verified/Likely/Possible/Unknown）+ 证据链 + 冲突标记 + Verified 目录继承；**真实库 4,245 条归属结论** |
| 任务四：识别中心页面 | ✅ 代码完成（编译通过；像素级截图未完成，见 §9.3） | 真实 IPC 数据渲染 + 空状态 + 规则启停 + 冲突展示 |
| 真实采样（ZCode/WorkBuddy/DSH） | ✅ 完成 | 见 §10（全部来自真实扫描数据） |
| 单文件交接 | ✅ | 本文件 |

数据库现状（真实库实测）：schema **v3** · tools 15 · rule_packs 1 · rules(enabled) 29 · **attributions 4,245** · paths **508,032**（历史数据零丢失）。

## 2. 开发前项目现状审查（第一阶段结论）

- Git：main @ f211601，工作区干净，5 提交（M0×1/M1×2/更名×1/M1收尾×1）。
- 规则表（tools/rule_packs/rules/attributions/agg_tool_size）在 v1 基线 schema 中**已存在**——M2 无需为表结构做大的重设计。
- 真实扫描数据（508,032 条）已含 `.dsh`×1、`.workbuddy`×6、`.codebuddy`×1、`.claude`×2、`.codex`×1、`Doubao`×1、**CLAUDE.md×1、AGENTS.md×3**——采样不需要产生新文件。
- M1 遗留技术债务 4 项均**不阻塞 M2**，已记录至 §15。

## 3. 本阶段目标与实际完成情况

见 §1 总览。补充差异说明：
- 规则表结构沿用现有设计（`rules.confidence` 承载规则级置信、新增 `weight/evidence/enabled` 三列承载引擎所需元数据）——未推翻重设计。
- 归属结论按"每节点×每工具一行"存储（支持冲突并存），而非单行单工具——为冲突透明化服务，属最小必要设计。

## 4. 关键技术方案与设计决策

| 决策 | 理由 |
|---|---|
| 规则以 JSON 数据文件随仓库分发（`rules/builtin-rules-v1.json`），加载进 DB | 数据与引擎分离；社区 PR 友好；不硬编码在前端 |
| 规则匹配 = **祖先名称链的后缀窗口匹配**（不解析 %USERPROFILE%） | 与"从卷根扫描"的现有架构解耦；对任意用户名通用；规则只写目录段（如 `.claude`、`AppData/Roaming/Cursor`） |
| 证据等级由**权重计算**而非人工指定：dir 规则 verified=1.0/likely=0.7；signal 自带 0.4~0.9；≥1.0=Verified、≥0.7=Likely、否则 Possible | 等级可复核；杜绝"路径命中就 Verified" |
| **互证升级**：信号命中节点若位于同工具数据目录内（祖先链窗口命中）→ Verified，双证据保留 | 例：.claude 内的 CLAUDE.md = 目录归属+信号归属互证 |
| **继承**：仅 Verified 目录归属向下传播（子节点无自配结论时），basis 记 `inherited/from_path_id` | 信号 Likely 不向下传播——避免把"工具在此工作过"夸大为"目录内文件都由 AI 生成"（任务书红线） |
| 归属 pass 为**独立可重跑的派生数据**：重跑前 `DELETE FROM attributions WHERE scan_id=?` | 派生数据可随规则升级重建，不触碰 paths/scans 历史表 |
| 冲突透明：同节点多工具同级别 → 每工具各存一行，basis JSON `conflict:true/conflict_with:[...]` | 不强行二选一，UI 可见 |

规则内容纪律：ZCode/WorkBuddy/DSH 因无官方路径文档，规则置信=likely、source 标注"本机观察/待验证"；`.vscode`、AGENTS.md 等通用痕迹仅给 possible 弱信号——不为凑数添加不可靠规则。

## 5. 数据库变更与兼容性

- **迁移 v3**（v2→v3，全部 ALTER ADD COLUMN 非破坏）：`rules.enabled INTEGER DEFAULT 1`、`rules.weight REAL`、`rules.evidence TEXT`、`tools.kind TEXT`。
- **真实库迁移实测**：508,032 条 paths 的 v2 库原地升 v3，paths 计数不变（§8 命令记录）。
- **事故与修复（如实记录）**：首次真实库迁移时 v3 的 tools.kind 列为后续补入，导致"版本号已到 3 但缺列"的半迁移状态——以幂等 ALTER 补列修复。教训：**版本号提升必须与完整 DDL 同批**（已写入 DEVELOPMENT_LOG）。
- identifier 未动；tracenest.db 未动位置；无任何历史数据删除。

## 6. 修改文件清单及作用

| 文件 | 作用 |
|---|---|
| `rules/builtin-rules-v1.json`（新） | 内置规则包数据（15 工具/29 规则，含来源 URL/证据/风险级/OS） |
| `crates/core/src/attribution.rs`（新） | 纯逻辑引擎：规则匹配（后缀窗口）、等级计算、互证升级、冲突检测（9 单测） |
| `crates/core/src/lib.rs` | 注册 attribution 模块 |
| `crates/database/src/rules_loader.rs`（新） | 内置规则包幂等加载 + set_rule_enabled |
| `crates/database/src/attribution_pass.rs`（新） | 归属 pass（树评估/继承/批量入库）+ 识别中心查询 API（overview/top_paths/build_full_path/rules/conflicts）+ 类型别名 |
| `crates/database/src/lib.rs` | v3 迁移 + BUILTIN_RULES_JSON + 模块注册 |
| `crates/database/examples/attribution_real.rs`（新） | 真实库归属运行器（验收与采样工具） |
| `apps/desktop/src-tauri/src/lib.rs` | setup：规则包加载；注册 commands_m2 |
| `apps/desktop/src-tauri/src/commands_m2.rs`（新） | 5 个 M2 IPC 命令（薄层） |
| `apps/desktop/src-tauri/src/commands.rs` | CommandError 构造函数 pub 化 |
| `apps/desktop/ui/src/pages/Tools.tsx`（新） | 识别中心真实页面 |
| `apps/desktop/ui/src/{lib/ipc.ts, App.tsx, pages/Placeholders.tsx}` | API/路由/占位页拆分 |
| `docs/M2_FINAL_HANDOFF.md`（本文件） | 单文件交接 |

## 7. 各项功能的实际运行情况

- 规则加载：真实库实测 `builtin v1.0.0: 15 工具 / 29 规则`，幂等重跑 skip ✅
- 信号检测：真实库发现 CLAUDE.md×1 → claude-code likely ✅（仅文件名匹配）
- 归属引擎：真实库 160,001 节点 **930ms** 完成；6 节点直配 + 4,239 继承 = 4,245 条结论 ✅
- 识别中心：代码完成、tsc/build 通过；数据层 SQL 与 example 完全一致（已真机跑通）——像素级 UI 截图未完成（§9.3）

## 8. 测试命令及真实测试结果

| 命令 | 结果 |
|---|---|
| `cargo test --workspace` | ✅ **37/37**：core 18（含归属引擎 9）+ database 16（含 scan_writer 一致性 8 + 迁移/恢复）+ platform 3 |
| `cargo clippy --workspace` | ✅ 0 警告 0 错误 |
| `cargo fmt --check` | ✅ |
| `npm run typecheck` / `build` / `test` | ✅ / ✅ / 7/7 |
| `cargo run -p tracenest-database --example attribution_real` | ✅ 真实库 930ms 完成归属；输出见 §10 |
| 只读 DB 校验（node:sqlite） | ✅ schema=3、tools=15、rules(enabled)=29、attributions=4,245、paths=508,032 |

## 9. Windows 实机验证情况

1. v3 迁移在真实库（v2，508K 路径）上完成 ✅（含一次半迁移事故与修复，§5）
2. 规则包加载 + 归属分析在真实库完成 ✅（930ms，结果 §10）
3. **UI 像素级截图未完成**：验证时应用窗口处于隐藏/未渲染状态，UIA 无法读取 WebView 内容（仅 70 元素）。代码层：Tools.tsx 通过 tsc/build，数据查询与 example 同源已验证。**待你启动应用人工确认识别中心显示**（启动方式：`cargo run` 或 npm dev + exe）。
4. 普通用户权限启动 ✅（提权拒绝逻辑不变）。

## 10. ZCode、WorkBuddy、DSH 实际采样结果（真实扫描数据）

| 工具 | 实际发现（实际证据） | 归属结果 | 状态 |
|---|---|---|---|
| **WorkBuddy** | `C:\Users\MR\.codebuddy`（393 KB，本扫描代真实命中） | **likely**（目录规则命中，双路径规则中先命中 .codebuddy） | 实际发现 ✅ |
| **DSH** | `C:\Users\MR\.dsh`（**277.6 MB**，真实命中——含 sessions/attachments 等） | **likely** | 实际发现 ✅ |
| **ZCode** | 当前扫描代无 `.zcode` 路径（C 盘扫描被取消于早期，未覆盖到该目录） | 未归属 | **尚未验证**（需重新全盘扫描覆盖） |
| Claude Code | `C:\Users\MR\.claude` + `settings.json` | **verified**（官方目录规则）+ CLAUDE.md 信号 likely | 实际发现 ✅ |
| OpenAI Codex | `C:\Users\MR\.codex` 子树（.tmp/plugins/.git 等，32.6 MB） | **verified ×4,241**（目录规则 + 继承） | 实际发现 ✅ |
| 豆包 | 规则已入库（AppData 双路径，本机验证） | 本扫描代未命中（该部分目录未覆盖） | 待全盘扫描 |

三态区分：上表"实际发现"= 真实扫描数据中的路径证据；"推测"= 规则语义（如 .workbuddy→腾讯品牌）；"未验证"= ZCode 规则与豆包命中（待全盘扫描后复核）。

## 11. 已验证的识别规则与尚待验证规则

- **已验证（verified 规则，有官方文档/本机证据）**：claude-code（.claude、.claude/projects、CLAUDE.md）、codex（.codex）、cursor（.cursor、AppData/Roaming/Cursor、.cursorrules、.cursorignore）、gemini-cli（.gemini、GEMINI.md）、vscode、ollama、huggingface——共 17 条
- **较可能（likely 规则，官方文档但本机未验证，或本机观察但无官方文档）**：windsurf×2、cline×2、roo-code×2、continue、zcode、workbuddy×2、dsh、doubao×2——共 10 条
- **弱信号（possible）**：AGENTS.md（跨工具开放约定）、.vscode——2 条
- 启用/禁用：`set_rule_enabled` IPC + DB enabled 列（UI 已有按钮）；规则版本：rule_packs.version + 归属 basis 记录 rules_version

## 12. 归属判断证据等级及其判定依据

- **Verified**：①命中官方文档确认的数据目录规则（weight 1.0）②目录规则+信号文件互证 ③位于已验证数据目录内的继承。判定依据链（rule_id、匹配描述、rules_version、judged_at）存于 attributions.basis JSON，UI 可展开。
- **Likely**：专属性信号（CLAUDE.md 0.9/.cursorrules 0.85/GEMINI.md 0.85）或 likely 级目录规则（0.7，如本机观察类）。
- **Possible**：弱信号（AGENTS.md 0.5/.vscode 0.4）。
- **Unknown**：无命中→不写 attributions 行→overview 中显示为"未知来源文件数"。
- **冲突**：同节点 ≥2 工具同级别 → 各行 basis 记 conflict+conflict_with，UI 显示冲突徽标；真实库本次 0 冲突（构造用例在单测覆盖）。

## 13. 隐私、安全与只读原则验收情况

- 全程仅使用 paths 表元数据；**零文件正文读取、零哈希、零会话/聊天记录访问**（代码路径上不存在）。
- 零网络出站；零用户文件修改（check-readonly 门禁保持通过）；未动注册表/环境变量/Defender/SAC。
- 信号文件识别不改变"只读元数据"承诺；未实现任何正文读取功能。
- 数据库操作仅限 FMap 自身 DB 的派生数据（attributions）与规则表——历史 paths/scans 未删改。

## 14. 未完成事项

1. 识别中心像素级截图（§9.3，数据层已验证）
2. ZCode 规则待全盘扫描覆盖后复核
3. `.cursorrules`/GEMINI.md 等信号在真实项目目录的命中验证（当前扫描未覆盖到含信号文件的项目根）
4. 规则启停后的**重新归属**提醒 UX（禁用规则需重跑分析生效）

## 15. 已知问题及技术债务

- M1 遗留 4 项（多实例检测/统计卡刷新/关闭钩子/NSIS 验证）→ 均不影响 M2，保留
- 归属 pass 为 O(N) 全节点评估（508K 节点实测 <1s，暂无性能问题）；增量归属（只算新扫描）列 M3
- 迁移纪律教训：版本号提升必须与完整 DDL 同批（§5 事故）
- 双实例并发扫描边界（M1 债务延续）

## 16. Git 提交记录与工作区状态

- main 分支，工作区干净（本次提交前）
- 本次 M2 提交：`feat(m2): AI 工具规则库+归属引擎+识别中心`（含 rules JSON、引擎、pass、IPC、UI、测试）
- 历史提交：5c1a304→7295afb→8a5d4b1→169c6ff→9b685ae→f211601→（本次）
- 无远程仓库；未推送；无密钥/隐私路径入库（DB 在用户 AppData，不在仓库）

## 17. M3 后续建议

1. **全量扫描补全**（用户授权后扫全盘）→ 直接补齐 ZCode/豆包/信号文件的真实验证
2. 磁盘地图/文件浏览页接入归属数据（treemap 按 AI 工具着色——M1 已预留 agg_tool_size）
3. 归属增量计算（只对新扫描代/新路径跑 pass）
4. 规则启停的重新分析提醒 UX；规则包本地导入（社区适配器）
5. M1 遗留债务清单（§15）逐项消化
6. 卸载安全清单报告（F9，数据已齐备：归属+风险级）

## 18. 给 ChatGPT 的审查摘要

FMap M2 完成了从"文件扫描器"到"AI 文件资产识别器"的升级：**15 工具/29 规则**的内置规则库（数据与引擎分离，可启停、有版本）；纯逻辑归属引擎实现 **Verified/Likely/Possible/Unknown** 四级证据分级（目录规则 1.0/0.7 权重、信号 0.4~0.9、目录+信号互证升级 Verified、冲突透明标记、仅 Verified 目录继承）；归属结果已对**真实扫描数据（508,032 条路径）**完成计算——4,245 条结论、930ms，其中 **.codex 子树 4,241 条 verified**、**.dsh（277.6MB）与 .codebuddy 真实命中 likely**、CLAUDE.md 信号 likely、ZCode 诚实标注未覆盖。测试 **37/37**（新增归属引擎 9 测）+ clippy 0 + fmt + 前端 7/7。DB v2→v3 非破坏迁移（4 列），历史数据零丢失。数据库现含 15 工具/29 规则/4,245 归属。未完成：识别中心 UI 像素截图（窗口隐藏态 UIA 限制，数据层已验证）、ZCode 待全盘扫描。隐私红线全程遵守（零正文/零哈希/零上传/零系统修改）。请基于本文件规划 M3（建议：全盘扫描补全 + 归属数据接入地图/浏览页 + 卸载安全清单）。
