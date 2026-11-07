# FMap — AI File Asset Tracker

> 让 AI 在电脑上留下的每一个文件，都有迹可循。
> Every file left behind by AI on your computer, accounted for.

FMap 是一款**本地运行、隐私优先、完全只读**的文件资产追踪工具（Windows 首发）。
它帮助使用 Claude Code、Codex、Cursor、Gemini CLI、ZCode 等 AI 工具的用户回答：

- AI 把项目/下载/缓存/模型放到了哪里？
- 每个 AI 工具实际占用了多少空间？
- 过去生成了什么？未来新产生了什么？
- 哪些文件发生了变化？哪些文件消失了（待确认）？
- 卸载某个 AI 工具时，哪些文件属于它（避免误删）？

## 核心能力

| 能力 | 里程碑 | 说明 |
|---|---|---|
| 只读多磁盘扫描 | M1 ✅ | 只读元数据遍历（不读文件正文），批量事务写入 |
| AI 工具识别 | M2 ✅ | 内置 15 工具/29 规则（目录规则 + 信号文件），四级证据归属引擎 |
| 文件身份 | M3-A ✅ | 卷 GUID + NTFS FileId 主身份（默认关闭，逐项授权） |
| 来源确认 | M3-A ✅ | 用户确认/修正/撤销（append-only），自动分析恒为 UNKNOWN |
| 基础事件追踪 | M3-B1 ✅ | 扫描差分：DISCOVERED / MODIFIED / MISSING / REAPPEARED |
| 实时目录监控 | M3-B2 ✅ | RDCW 逐目录授权（默认关闭），溢出自动对账 |
| USN Journal | M3-E 📋 | 规划完成，待实施 |

## 隐私与安全承诺

- **零网络出站**：不上传任何文件路径、名称、内容或报告；无账号、无 API Key
- **只读**：不删除、不修改、不移动用户文件（CI 有只读门禁）
- **不读正文**：只使用文件元数据（路径/大小/时间/身份）
- **不算哈希**：默认不计算文件内容哈希
- **不提权**：设计上拒绝管理员运行
- **来源不编造**：文件来源行为只能由用户手动确认；自动分析永远显示 UNKNOWN

## 技术架构

```
Tauri 2 壳 (React 19 + TypeScript + Tailwind v4)
    ├── IPC 命令层
    ├── crates/core        # 扫描引擎 / 归属引擎 / 路径规范化（OS 无关纯逻辑）
    ├── crates/database    # SQLite (WAL + FTS5) / 迁移 / 差分 / 归属 / 监控
    └── crates/platform    # Windows 磁盘发现 / 文件身份 / RDCW 监控
```

## 快速开始

### 前置条件

- Windows 10/11
- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 1.70+ (MSVC target)
- [VS Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (C++ workload)

### 开发运行

```bash
git clone https://github.com/YOUR_USERNAME/FMap.git
cd FMap
npm install --prefix apps/desktop/ui
npm run dev --prefix apps/desktop/ui   # 终端 1：Vite dev server
cargo tauri dev                        # 终端 2：Tauri 开发模式
```

### 发布构建

```bash
npm run build --prefix apps/desktop/ui
cargo build --release -p tracenest-desktop --features custom-protocol
```

## 测试

```bash
cargo test --workspace              # Rust 全量测试（85+）
cargo clippy --workspace            # 静态分析
npm test --prefix apps/desktop/ui   # 前端测试
node tests/check-readonly.mjs       # 只读安全门禁
node tests/validate-schema.mjs      # Schema 验证
```

## License

MIT OR Apache-2.0（双许可）
