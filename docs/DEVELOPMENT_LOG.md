# 开发日志（DEVELOPMENT_LOG）

> 约定：每次重要开发操作追加一节；保持倒序（最新在上）。

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
