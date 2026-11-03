# 参与贡献（CONTRIBUTING）

感谢关注 TraceNest！项目目前处于 M0→M1 阶段，以下约定请先读一遍。

## 开发环境

- Windows 10 21H2+ / Windows 11（首发平台）
- Node.js ≥ 20、npm ≥ 10、Git
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
node tests/validate-schema.mjs    # DDL 真实验证
node tests/check-readonly.mjs     # 只读门禁（CI 必过）
```

## 提交规范

- Conventional Commits（`feat:` / `fix:` / `docs:` / `chore:` / `test:`）
- 一个 PR 一个主题；带测试的改动请附测试结果

## 硬性红线（PR 会被直接拒绝）

1. **只读承诺**：任何删除/移动/重命名用户文件的代码（CI 有 grep 门禁）
2. **无提权**：任何需要管理员权限的 API（如 MFT 直读、ETW）
3. **零网络出站**：任何遥测、上报、自动更新、第三方 AI API 调用
4. **隐私默认关闭**：读取会话/聊天/文件正文的功能必须默认关闭且逐项授权
5. **不伪造数据**：UI 不得用演示数据冒充真实扫描结果

## 提问与反馈

- Bug / Feature：GitHub Issues（仓库公开后）
- AI 工具目录适配：见 docs/research/AI_TOOLS_DIRECTORIES.md 的规则包格式（M2 交付适配器 SDK）
