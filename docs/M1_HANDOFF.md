# M1 交接文档（M1_HANDOFF）

> 写给：下一轮开发（或工具链安装后的第一次会话）· 日期：2026-11-03

---

## 1. 当前项目状态

- Git：main 分支，1 个初始提交（M0 骨架 + 全部研究文档）。无远程仓库（按指令不创建/不推送）。
- 可运行部分：前端（构建/测试/渲染均已实测）；两个 Node 层测试脚本（schema 验证、只读门禁）。
- 待激活部分：全部 Rust 代码（core/database/platform/src-tauri）——**已编写、未编译**。

## 2. 当前代码结构

```
FMap/
├── Cargo.toml                     # workspace：crates/* + apps/desktop/src-tauri
├── crates/
│   ├── core/       # ScanStatus 七态、CoreError、format_bytes（纯逻辑，带测试）
│   ├── database/   # Database::open/migrate/upsert_volume/begin_scan/finish_scan/stats
│   └── platform/   # FileSystemProvider trait、VolumeInfo、Windows 磁盘发现、提权检测
├── apps/desktop/
│   ├── src-tauri/  # Tauri 壳：4 个 IPC 命令、AppState、提权拒绝(exit 87)、CSP
│   └── ui/         # React 19：9 页导航、ipc.ts 封装（浏览器模式降级）、真实状态首页
├── tests/          # schema.sql（权威 DDL）、validate-schema.mjs、check-readonly.mjs
├── scripts/        # make-icon.mjs
└── docs/           # 全部研究/架构/开发文档（勿覆盖）
```

## 3. M0 已完成 / 未完成

已完成：见 M0_COMPLETION_REPORT §1。
未完成：Rust 编译验证、桌面应用真机运行、IPC 端到端、设置持久化——全部依赖工具链安装。

## 4. 测试情况

已真实通过：前端 vitest 7/7、schema 验证、只读门禁、tsc、vite build、浏览器渲染验证（证据见 M0_COMPLETION_REPORT §3）。
待执行：`cargo test`（13 个 Rust 单测已写好：core 3、database 6、platform 4，其中 platform 的列卷测试会在真机上真实枚举磁盘）。

## 5. 工具链就绪后立即执行（按序）

```powershell
# 1) 人工安装（约 30-60 分钟，勿在本会话自动执行）
winget install --id Rustlang.Rustup -e            # 或 https://rustup.rs
winget install --id Microsoft.VisualStudio.2022.BuildTools -e
#    ↑ 安装器中勾选"使用 C++ 的桌面开发"工作负载（含 MSVC v143 + Windows 11 SDK）

# 2) 验证
rustc --version && cargo --version

# 3) 首次编译 + 全部 Rust 测试（在 FMap/ 下）
cargo clippy --workspace -- -D warnings
cargo test --workspace          # 13 个单测 + Windows 真机列卷测试
cargo fmt --all

# 4) 桌面应用真机运行（IPC 端到端验收）
cd apps/desktop/ui && npm install && npm run build
cd ../src-tauri && cargo tauri dev   # 或 npm install -D @tauri-apps/cli 后 npx tauri dev
```

预期问题与处理：
- windows crate API 签名可能与我手写版本有出入（版本差异）→ 按 clippy/rustc 报错微调，**不要改接口设计**。
- rusqlite bundled 首次编译较慢（编 SQLite C 源，数分钟）→ 正常。
- `cargo tauri dev` 前需 `npm i -D @tauri-apps/cli`（未预装，遵循按需引入）。

## 6. 下一阶段任务（M1：扫描引擎）

按 DEVELOPMENT_PLAN.md M1 里程碑：
1. `crates/core` 增加 scanner 模块：多线程目录遍历（std::fs 起步），七态状态机接入 `VolumeScanState`。
2. 批量写库：paths 表（事务 5k–10k 行/批）、目录 size 自底向上聚合。
3. 扫描命令：`scan_start/scan_cancel/scan_status` + 进度事件（Tauri event → 前端磁盘列表实时状态）。
4. 权限不足：逐目录捕获 Access Denied 计数，绝不中断整卷（需求六.6/.7）。
5. 合成文件树基准（10 万文件）：真实测量吞吐，**不凭空宣称速度**。
6. 性能达标后：`GetFileInformationByHandleEx(FileFullDirectoryInfo)` 优化路径（保持 trait 不变）。

## 7. 技术风险（M1 相关）

- windows crate 手写 API 签名的编译期修正（预期小）。
- 深层 node_modules 路径（>260 字符）——std::fs 在 Win10+ 通常已支持长路径，需真机验证；不支持则切 `\\?\` 前缀。
- 扫描期间 UI 查询并发：WAL 已开，注意批量事务大小。

## 8. 需要人工处理的事项

1. **安装 Rust 工具链 + MSVC Build Tools**（唯一阻塞项，见 §5）。
2. 决定是否创建 GitHub 远程仓库（本项目暂无远程；推送需你授权）。
3. （可选）正式 Logo 替换：改 scripts/make-icon.mjs 或提供 icon.ico。
4. （可选）`npm install-scripts approve esbuild` 处理 npm 新版脚本提示。
