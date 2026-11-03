# 更名报告：TraceNest → FMap（RENAME_TO_FMAP_REPORT）

> 日期：2026-11-04 · 性质：仅品牌更名，无功能变更、无 M2 内容

## 1. 名称对照

| 位置 | 前 | 后 |
|---|---|---|
| 窗口标题栏（tauri.conf.json） | TraceNest — AI 文件资产地图 | **FMap — AI 文件资产地图** |
| 后端 APP_NAME（crates/core） | TraceNest | **FMap**（顶栏"应用卡/顶栏名称"数据源） |
| 侧栏 Logo 徽标/名称（Layout.tsx） | TN / TraceNest | **FM / FMap** |
| 权限提示（提权拒绝 eprintln） | [TraceNest] | [FMap] |
| productName / bundle 元数据 | TraceNest | FMap |
| 应用图标字形 | N | **F**（make-icon.mjs 已重生成 icon.ico） |
| index.html / 浏览器页标题 | TraceNest — | FMap — |
| 首页"权限模式"卡描述 | TraceNest 设计上拒绝… | FMap 设计上拒绝… |

## 2. 修改文件清单

**代码/配置（17）**：tauri.conf.json、src-tauri/{Cargo.toml,src/lib.rs,src/main.rs}、ui/index.html、ui/src/{components/Layout.tsx,pages/Overview.tsx,pages/Placeholders.tsx,__tests__/ipc.test.ts}、crates/core/{Cargo.toml,src/lib.rs,examples/scan.rs}、crates/platform/{Cargo.toml,src/lib.rs}、crates/database/{Cargo.toml,src/lib.rs}、tests/check-readonly.mjs、scripts/make-icon.mjs + 重新生成的 icons/icon.ico
**文档（23）**：README、CONTRIBUTING、SECURITY、docs/ 下 PRD/ROADMAP/全部研究/架构/开发/交接文档（精确替换 TraceNest→FMap，保留小写技术标识）

## 3. 用户可见界面检查结果（实机截图验证）

启动应用实测：窗口标题栏 ✓ FMap、首页 Logo+FMap ✓、侧栏 ✓、顶栏"后端已连接"旁 ✓ FMap v0.1.0 · windows、总览页各卡 ✓、空状态标注 ✓。托盘：产品无托盘功能（不适用）；安装包：未打包（bundle 时将产出 FMap 命名，配置已生效）。

## 4. 构建/测试结果（全部真实执行）

- 前端：tsc ✅ · vite build ✅ · vitest 7/7 ✅
- Rust：cargo test --workspace **17/17** ✅（core 9 + database 5 + platform 3）
- 更名前发现"旧二进制残留旧标题"问题 → 重编译后消除（tauri 配置/APP_NAME 编译期嵌入）

## 5. 剩余 TraceNest 字样及理由

| 位置 | 理由 |
|---|---|
| docs/DEVELOPMENT_LOG.md、docs/M1_PROMPT_FROM_GPT.md | 历史档案，已加"更名前（TraceNest）"标注，不篡改 |
| identifier `io.github.tracenest.desktop` | **数据兼容性决策（见 §6）** |
| crate 名 tracenest-*、DB 文件名 tracenest.db、模块/IPC 名 | 指令明确禁止重命名内部技术标识 |
| 本地目录 TraceNest/、git 历史 | 指令禁止重命名本地目录与仓库 |

## 6. 数据库/配置目录/历史数据影响分析

**结论：无影响，历史数据完好。** 关键决策：**保留 identifier 不变**。原因：应用数据目录 = %APPDATA%\io.github.tracenest.desktop\tracenest.db，其中存有 M1 验收的 **326,521 条真实扫描路径**；改 identifier 会让 FMap 指向新的空目录，等效于用户数据丢失（违反"不破坏原有数据"）。若未来必须改，需附带数据迁移逻辑，作为独立任务评审。实机验证：更名后启动应用，"扫描记录"显示 总数 1 / 已索引路径 326,521 —— 历史扫描数据完整可读。

## 7. 尚未验证的内容

- NSIS 安装包产物名（需 bundle 打包验证，配置已正确：productName=FMap）
- 正式 Logo 设计（当前为程序化 F 字形占位，与原 N 同等地位）
- macOS/Linux 下的标题行为（仅 Windows 验证，与更名无关）
