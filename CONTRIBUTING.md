# 参与贡献（CONTRIBUTING）

感谢关注 FMap！项目已完成 M0→M4（扫描 / 识别 / 事件追踪 / 行为关联 / 浏览器下载 / 磁盘地图），
正在向产品化阶段打磨。无论你是写代码还是只想补一条规则，这里都有适合你的参与方式。

## 三分钟选一条参与轨道

| 你是… | 推荐轨道 | 做什么 |
|---|---|---|
| 用过某个 AI 工具的人 | 🧩 规则包贡献 | 在 `rules/builtin-rules-v1.json` 里为你的工具补充数据目录 / 特征文件规则——**只需 JSON，不需要会 Rust** |
| 普通用户 | 🐛 Issue 反馈 | 扫描结果不对、识别遗漏、界面问题 → [开 Issue](https://github.com/Yangxbao/FMap/issues/new)，附上截图（请隐去个人路径） |
| Rust / TS 开发者 | 🦀 代码贡献 | 从 `good first issue` 挑任务，先读完本文件的硬性红线 |
| 写作者 | 📖 文档 / 翻译 | 完善文档、英文 README |

### 🧩 规则包贡献（最容易的方式）

1. Fork → 编辑 `rules/builtin-rules-v1.json`；
2. 为你的 AI 工具添加条目：`kind: "dir"`（数据目录模板，如 `.mytool`）或
   `kind: "signal"`（特征文件名，如 `MYTOOL.md`），并写明 `source`（依据链接）；
3. 跑 `node tests/validate-schema.mjs` 确认 JSON 合法 → 提 PR。

规则是纯数据文件，引擎会自动加载——这是帮所有用户识别新工具的最快路径。

## 开发环境

- Windows 10 21H2+ / Windows 11（首发平台）
- Node.js ≥ 22.5（`node:sqlite` 用于仓库级测试）、npm ≥ 10、Git
- Rust stable（msvc 目标）+ MSVC Build Tools（含 Windows SDK）
- WebView2 Runtime（Win10/11 一般自带）

## 常用命令

```bash
# 前端（apps/desktop/ui）
npm install
npm run typecheck        # tsc --noEmit
npm test                 # vitest
npm run build            # tsc + vite build

# Rust（仓库根）
cargo clippy --workspace -- -D warnings
cargo test --workspace

# 仓库级安全/结构测试（无需 Rust 工具链）
node tests/validate-schema.mjs    # DDL 真实验证（需要 Node ≥ 22.5）
node tests/check-readonly.mjs     # 只读门禁（CI 必过）
```

CI 会在 windows-latest 上跑全部以上检查——本地先过再提 PR 最省时间。

## 提交规范

- Conventional Commits（`feat:` / `fix:` / `docs:` / `chore:` / `test:`）
- 一个 PR 一个主题；带测试的改动请附测试结果（命令 + 输出，不截图伪造）

## 硬性红线（PR 会被直接拒绝）

1. **只读承诺**：任何删除/移动/重命名用户文件的代码（CI 有 grep 门禁）
2. **无提权**：任何需要管理员权限的 API（如 MFT 直读、ETW）
3. **零网络出站**：任何遥测、上报、自动更新、第三方 AI API 调用
4. **隐私默认关闭**：读取会话/聊天/文件正文/浏览器数据的功能必须默认关闭且逐项授权
5. **不伪造数据**：UI 不得用演示数据冒充真实扫描结果；未知不得包装成结论

## 提问与反馈

- Bug / Feature：[GitHub Issues](https://github.com/Yangxbao/FMap/issues)
- 设计讨论：先读 `docs/architecture/` 与 `docs/research/` 里的既有决策（尤其是三层分模型），欢迎在 Issue 里挑战
