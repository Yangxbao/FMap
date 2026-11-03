# 本机开发环境检查报告（ENVIRONMENT_CHECK）

> 检查时间：2026-11-03（夜间）· 方式：只读命令（`Get-CimInstance` / `reg query` / `--version`），**未安装、未修改任何系统配置**
> 结论：**Node.js / WebView2 / Git 已就绪；Rust 工具链与 MSVC C++ Build Tools 缺失**（Tauri 2 Windows 开发的两个硬依赖），已附官方安装链接，待用户明早决定。

---

## 1. 硬件与系统

| 项目 | 结果 | 评估 |
|---|---|---|
| 操作系统 | Microsoft Windows 11 家庭版 中文版，Build 26200（x64） | ✅ 满足 Tauri 2（需 Win10 21H2+） |
| CPU | AMD Ryzen 7 7435H，8 核 16 线程 | ✅ 扫描引擎多线程无压力 |
| 内存 | 15.7 GB | ✅ 足够（含 Rust 编译与 100 万文件级 DB 测试） |
| C 盘（系统） | 199 GB，剩余 62.3 GB | ⚠️ 够用但不算宽裕；Rust 工具链 + target 目录建议关注空间（预估 Rust 环境约 2–4 GB） |
| D 盘（软件） | 276.6 GB，剩余 250.6 GB | ✅ 适合作为项目/工具链安装位置（若用户愿意自定义） |
| E 盘（文档） | 953.9 GB，剩余 405 GB | ✅ 数据盘 |

## 2. 开发工具链

| 工具 | 状态 | 版本 | Tauri 2 需要 |
|---|---|---|---|
| Node.js | ✅ 已安装 | v24.21.0 | ≥ 18 ✅ |
| npm | ✅ 已安装 | 11.19.0 | ✅ |
| Git | ✅ 已安装 | 2.55.0.windows.5 | ✅ |
| WebView2 Runtime | ✅ 已安装 | 156.0.4314.58（注册表 EdgeUpdate\Clients\{F3017226-...} 验证） | ✅（Windows 运行时必需） |
| winget | ✅ 已安装 | v1.29.380 | 可用于补装依赖 |
| Rust (rustc/cargo/rustup) | ❌ 未安装 | — | **必需**（Rust 核心） |
| pnpm / yarn | ❌ 未安装 | — | 非必需（npm 可替代） |
| MSVC C++ Build Tools (cl.exe / link.exe) | ❌ 未安装（vswhere、VS 目录、Windows SDK 均未找到） | — | **必需**（Rust msvc 目标链接器） |
| Windows SDK | ❌ 未安装 | — | 随 Build Tools 一起装 |

## 3. 缺失依赖的安装建议（**仅建议，未执行**）

Tauri 2 Windows 前置条件（官方文档：https://tauri.app/start/prerequisites/ ）：

1. **Rust（含 rustup、stable-msvc 目标）**
   - 官方：https://rustup.rs 或 https://www.rust-lang.org/tools/install
   - 或 winget：`winget install --id Rustlang.Rustup -e`（版本以 winget 源为准）
   - 安装后确认：`rustc --version`（stable 通道 + `x86_64-pc-windows-msvc` 目标）
2. **Microsoft C++ Build Tools（含 MSVC v143 + Windows 11 SDK）**
   - 官方：https://visualstudio.microsoft.com/visual-cpp-build-tools/
   - 或 winget：`winget install --id Microsoft.VisualStudio.2022.BuildTools -e`（装完后在 Visual Studio Installer 中勾选 "使用 C++ 的桌面开发" 工作负载）
3. **WebView2**：已就绪，无需操作。
4. （可选）pnpm：`winget install pnpm`——本项目用 npm 也完全可以。

> 提示：两条 winget 命令 + Rust 安装器约需 30–60 分钟与 2–4 GB 磁盘空间（建议关注 C 盘剩余 62 GB，问题不大）。

## 4. 对开发计划的影响

- M0 里程碑（环境与骨架）的安装项即上述 1、2 两条，装完即可 `create-tauri-app` 跑通。
- 其余全部就绪；**今晚未做任何安装**（遵守约束）。
- 本报告的所有检查命令均为只读；未扫描任何用户文件内容，未访问聊天记录。
