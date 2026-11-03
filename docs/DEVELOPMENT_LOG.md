# 开发日志（DEVELOPMENT_LOG）

> 约定：每次重要开发操作追加一节；保持倒序（最新在上）。

---

## 2026-11-03 · 跨工具协作：GPT 评审 + M1 扫描引擎（主会话执行，无子代理）

**开发模块**：电脑操控（代发文档给 GPT）、扫描引擎、数据库 DAO、前端扫描界面、测试

**主要修改**：
1. 应用户要求，通过电脑操控在其 Edge 浏览器中找到既有 ChatGPT 会话"AI文件管理项目调研"，发送 M0_COMPLETION_REPORT.md + M1_HANDOFF.md 全文（剪贴板粘贴，4259 字符），GPT 完成评审并生成《TraceNest V0.1 — M1 扫描引擎正式开发指令》（全文存档：docs/M1_PROMPT_FROM_GPT.md）。
2. GPT 提示词包含 13 步执行顺序 + 10 项交付要求 + 3 个验收问题（Rust 有没有编译/扫描结果是否来自真实磁盘/取消后数据库一致性）+ 红线（不伪造/不自动安装/无子代理/不碰个人文件）。
3. 按 13 步执行：M1_PRECHECK.md（步骤 1-5）→ scanner.rs 扫描引擎（只读遍历、七态、取消、排除目录、链接不跟随、6 单测）→ database 批量 DAO + 目录 size 自底向上聚合 → src-tauri scan_start/status/cancel 后台线程命令 → 前端扫描卡（磁盘勾选、1.5s 轮询、部分完成明示）→ fixture 生成器。
4. 浏览器操控备注：ChatGPT 页面流式渲染导致 UIA surface 反复重建（surface_replaced），滚动/复制多次失败；最终以辅助功能树读取回复全文解决。经验：对持续重渲染的页面，优先读 AX 树文本而非视口操作。

**测试命令与结果**：
- `node tests/make-fixture.mjs 200 5` → ✅ 400 目录/1000 文件确定性树
- `node tests/check-readonly.mjs` → ✅（含新 scanner.rs）
- `node tests/validate-schema.mjs` → ✅
- `npm run typecheck` / `build` → ✅（新扫描 UI 通过 strict）
- `npm test` → ✅ 7/7
- `cargo test`（14 单测）→ ⏸ 未执行（工具链缺失，诚实标记）

**遇到的问题与解决**：
| 问题 | 解决 |
|---|---|
| 首次粘贴进 ChatGPT 输入框未生效（焦点丢失） | 改为"真实鼠标点击输入框(strategy=event) + 系统剪贴板 Ctrl+V" |
| 点击发送按钮无响应 | 改为聚焦输入框后按 Return 发送 |
| UIA surface_replaced 反复出现 | 同单元格"观察+操作"；最终放弃视口操作改读 AX 树 |
| scan_start 引用未实现的 DB 方法 | 补 query_latest_running_scan / ensure_volume |

**遗留**：Rust 编译验证（唯一阻塞，人工安装步骤见 M1_HANDOFF §5）；真机扫描对账与性能基准。

---

## 2026-11-03 · M0 基础开发（主会话执行，无子代理）

**开发模块**：项目骨架、Rust workspace、Tauri 壳、React 前端、测试体系、开源合规文件

**主要修改**：
1. 阶段一检查：确认 17 项研究成果在位；识别 6 个问题（结构调整、9 项导航、决策落定等），处理方式见 FINAL_PRE_DEVELOPMENT_CHECK §1.2。
2. 环境复检：Node 24.21 ✅ / npm 11.19 ✅ / Git 2.55 ✅ / WebView2 156 ✅ / **Rust ❌ / MSVC ❌**（按指令未安装）；发现 node:sqlite 可用 → 决定用它做 schema 真实验证。
3. 结构搭建：workspace（core/database/platform + src-tauri）、LICENSE-MIT/APACHE、.gitignore。
4. Rust 代码：core（七态 ScanStatus/CoreError/format_bytes）、database（迁移 + 卷/扫描 CRUD + stats）、platform（FileSystemProvider trait + Windows 磁盘发现 + 提权检测）、src-tauri（4 命令 + AppState + CSP）。
5. 前端：9 页导航壳（含里程碑徽标）、ipc.ts（浏览器模式显式降级）、Overview 真实状态页、Settings 决策展示页、空状态组件。
6. 测试与验证：
   - `node tests/validate-schema.mjs` → ✅（17 表、FTS5、外键、冒烟读写）
   - `node tests/check-readonly.mjs` → ✅（无危险 API）
   - `npm run typecheck` → 首次发现 1 个未使用导入（Layout.tsx PAGE_IDS）→ 修复 → ✅
   - `npm run build` → ✅ 38 模块 / dist 250KB
   - `npm test` → 首次 5/7（IPC 测试 mock 挂错位置：@tauri-apps/api 读 `window.__TAURI_INTERNALS__.invoke`）→ 修正 mock → ✅ 7/7
   - 浏览器打开 dist → ✅ 9 导航、诚实"后端未连接"、无 DEMO 数据、设置页截图核对
7. 图标：scripts/make-icon.mjs 程序化生成 icon.ico（8254 字节，正式 Logo 待换）。
8. Git：init(main) + 初始提交 58 文件（LF→CRLF 提示为 Windows 正常现象）。
9. 文档：FINAL_PRE_DEVELOPMENT_CHECK / M0_COMPLETION_REPORT / M1_HANDOFF / 本日志 / README / CONTRIBUTING / SECURITY(根)。

**测试命令与结果**：见 M0_COMPLETION_REPORT §3 表格。

**遇到的问题与解决**：
| 问题 | 解决 |
|---|---|
| tsc 报 TS6133 未使用导入 | 删除 Layout.tsx 的 PAGE_IDS 导入 |
| vitest 2 例失败：mock 未挂到 `window.__TAURI_INTERNALS__.invoke` | 重写测试 mock 路径，7/7 通过 |
| Windows 路径分隔符（normalize）导致静态服务器 403（昨日原型阶段同款问题） | ROOT 统一 path.normalize |

**遗留**：Rust 编译/运行待工具链（唯一阻塞项，步骤见 M1_HANDOFF §5）。
