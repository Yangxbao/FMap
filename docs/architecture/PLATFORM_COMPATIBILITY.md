# FMap 跨平台兼容性分析（PLATFORM_COMPATIBILITY）

> 版本：v0.9（夜间研究稿） · 日期：2026-11-03
> 结论标注：✅=已有证据（TECH_FEASIBILITY 引用来源）｜❓=待验证｜⚠️=有已知限制
> 最终能力边界以 [TECH_FEASIBILITY.md](../research/TECH_FEASIBILITY.md) 的引用为准，本文档汇总为实施矩阵。

---

## 1. 平台支持矩阵（功能 × 平台）

| 能力 | Windows 10 21H2+/11 | macOS 13+ | Linux（X11/Wayland） |
|---|---|---|---|
| 目录元数据扫描（std::fs / read_dir） | ✅ | ✅ | ✅ |
| 递归实时监控（notify） | ✅ ReadDirectoryChangesW | ✅ FSEvents | ⚠️ inotify（见 §4） |
| 文件名 FTS 搜索 | ✅ | ✅ | ✅ |
| AI 工具目录知识库 | ✅（规则最全） | ⚠️ 部分工具差异 | ⚠️ 部分工具差异 |
| 每文件 USN 记录（parent 链） | ✅ FSCTL_READ_FILE_USN_DATA（❓无管理员确认） | N/A | N/A |
| 全盘 USN/MFT 枚举 | ❌ 需管理员，**不做** | N/A | N/A |
| 事件进程归因（ETW/kqueue/fanotify） | ❌ 需管理员，**不做** | ❌ 需权限，**不做** | ❌ 需 root，**不做** |
| Tauri 2 壳 | ✅ WebView2 | ✅ WKWebView | ✅ WebKitGTK |
| 应用商店沙盒 | N/A | ⚠️ App Store 版受限（自分发为主，避开沙盒） | N/A |

---

## 2. Windows（主平台）

**环境**：WebView2 Evergreen 运行时（Win10 21H2+/Win11 系统自带，缺失时 Tauri 提供引导下载——正式版需确认是否捆绑 bootstrapper，涉及用户决策）。

**要点**：
- **长路径**：`\\?\` 前缀处理与 260 字符限制；`node_modules` 深路径常见 → 扫描器必须启用长路径感知（std::fs 在 Win10 起多数 API 已自动支持，需测试验证 ❓）。
- **reparse points**：junction（如 pnpm store、OneDrive/坚果云同步目录）、符号链接 → 不跟随、记录目标；OneDrive 按需文件（placeholder）的 size 语义 ⚠️ 需要专门处理（显示"云端文件"标记而非下载）。
- **权限错误**：`ProgramData`、其他用户目录等会遇 `Access Denied` → 逐项跳过并计数，UI 显示"部分目录无权读取（N 项）"。
- **网络盘/WSL 路径**：默认排除（`\\server\share`、`\\wsl$\`），可配置。
- **事件监控**：ReadDirectoryChangesW 递归单句柄即可，但缓冲区溢出会丢事件 → notify 层处理 + overflow 计数上报 ⚠️。
- **盘符热插拔**：U 盘/移动硬盘 → 扫描与 watch 需响应设备到达/移除事件（❓V0.1 范围外，先做"扫描时卷不存在"的优雅降级）。

## 3. macOS（次优先）

- **FSEvents**：无需管理员即可 watch；per-file 粒度需 `kFSEventStreamCreateFlagFileEvents`；存在合并/延迟（coalescing）⚠️。
- **TCC 权限**：首次访问 `~/Desktop`、`~/Documents`、`~/Downloads` 及外置卷会触发系统授权弹窗 ⚠️——FMap 的"用户显式选择扫描目录"流程天然契合（用户选择 = 授权路径）。
- **非沙盒分发**（直接 GitHub 下载 .app）避免 App Store 沙盒限制；自分发 + 公证（notarization）需要 Apple 开发者账号（预算决策，交用户）。
- **目录差异**：多数 CLI 工具用 `~/.xxx`（与 Linux 一致），桌面应用在 `~/Library/Application Support`。

## 4. Linux（架构兼容）

- **inotify**：无递归 watch → notify 需为每目录建 watch；默认 `fs.inotify.max_user_watches`（典型 8192–524288，发行版差异 ❓）在大型项目树会耗尽 → watch 限额 + 降级提示（"监视部分目录"）。
- **fanotify** 递归/挂载级监控需 root → 不用。
- **挂载点**：跨文件系统扫描需防循环（`/proc`、`/sys`、bind mount）→ 挂载表驱动扫描。
- **发行版打包**：AppImage / deb / rpm（V1.0）。

---

## 5. AI 工具目录的跨平台差异（对规则库的影响）

同一工具在不同 OS 的目录差异直接影响归属准确率：

| 工具 | Windows | macOS / Linux |
|---|---|---|
| Claude Code | `%USERPROFILE%\.claude`（+ `%LOCALAPPDATA%` 变体❓） | `~/.claude`（一致） |
| Codex | `%USERPROFILE%\.codex` | `~/.codex` |
| Cursor | `%APPDATA%\Cursor`（Chromium 数据）+ `~/.cursor` | `~/Library/Application Support/Cursor` + `~/.cursor` |
| 项目根信号文件 | 全平台一致（CLAUDE.md/AGENTS.md/.cursorrules） | 全平台一致 |

> 结论：**信号文件规则天然跨平台**，目录规则需按 OS 维护——这就是规则包 schema 带 `os` 字段的原因。详细路径与状态见 [AI_TOOLS_DIRECTORIES.md](../research/AI_TOOLS_DIRECTORIES.md)。

---

## 6. 开发优先级结论

1. V0.1：Windows 单平台打磨（本机环境已验证 Node/WebView2 就绪，缺 Rust 工具链——见 ENVIRONMENT_CHECK）。
2. V0.2–V0.3：规则库的 OS 维度完善（数据工作，非架构工作）。
3. V1.0：macOS/Linux 壳验证（Tauri 2 本身跨平台成熟，主要成本在 FSEvents/inotify 行为差异与打包签名）。
