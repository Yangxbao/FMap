# FMap 最终发布报告（FMAP_FINAL_RELEASE_REPORT）

> **日期**：2026-11-07 · **版本**：v0.1.0 · **状态**：已发布到 GitHub
> **仓库**：https://github.com/Yangxbao/FMap (Public)

---

## 1. 最终版本与 Git

| 项 | 值 |
|---|---|
| 版本 | v0.1.0 |
| 分支 | main |
| HEAD | `b8ab822` |
| 标签 | `v0.1.0` → `b8ab822` |
| 远程 | `https://github.com/Yangxbao/FMap.git` |
| 可见性 | Public |
| 工作区 | 干净 |
| 完整提交数 | 25+（M0→M3-B2 全程） |

## 2. 完成的全部里程碑

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** | 项目骨架（Tauri 2 + Rust workspace + React 19 + SQLite） | ✅ |
| **M1** | 只读多磁盘扫描引擎（七态状态机 + 批量事务 110K 行/秒 + 覆盖缺口收集） | ✅ |
| **M2** | AI 工具识别（15 工具/29 规则 + 四级证据归属引擎 + 识别中心 UI） | ✅ |
| **M3-A** | 文件身份（卷 GUID + NTFS FileId）+ 来源确认三层分模型 + 实机验收 | ✅ |
| **M3-B1** | 基础文件事件追踪（扫描差分四类事件 + 台账 + 覆盖缺口抑制） | ✅ |
| **M3-B2** | RDCW 实时目录监控（逐目录授权 + 原始通知层 + 溢出对账闭环） | ✅ |
| **M3-C** | AI 工具行为关联（时间关联候选） | 📋 规划完成，待后续版本 |
| **M3-D** | 浏览器下载记录集成 | 📋 规划完成，待后续版本（依赖用户逐项授权） |
| **M3-E** | USN Journal + 后台监视 + 跨平台 | 📋 规划完成，待实施 |

## 3. 多根扫描问题修复

**根因**：M1 将多根扫描的全部 paths 挂到第一根的卷（`volume_id` 取 roots[0]），
导致卷归属与实际路径不一致。

**修复**：`scan_start` 重构为**每根独立扫描代 + 串行循环 + 聚合进度**：
- 每根独立 `begin_scan`（roots 只含本根——scope 一致性前提）
- 每根独立 `run_scan_to_db`（paths 挂自己的卷）
- 每根独立自动差分（仅本卷）
- 聚合进度（files/dirs/denied 跨根累计 + phase 显示"卷 1/N"）
- 失败隔离：一卷失败记 partial 并继续下一根；取消跳过剩余根
- 已有的混挂历史数据：差分卷隔离 + scope 校验双重防御（真实环境验证）

**测试**：database 56/56 含多根/部分失败/取消/重试覆盖。

## 4. B2 RDCW 完整实现

| 组件 | 实现 |
|---|---|
| **授权管理** | watch_roots 表（规范化路径 + 歧义拒绝 + 嵌套去重）；默认关闭；启停/撤销 |
| **原始通知层** | ReadDirectoryChangesW overlapped 采集 → events 表（零去重，含系统重复通知） |
| **溢出对账** | ERROR_NOTIFY_ENUM_DIR 检测 → gap 开启 → 自动触发所属登记根扫描 → 差分补齐 → 关闭判据（对账差分完成 ∧ 水位排空 ∧ 无新溢出） |
| **监控状态** | 四层水位（接收/持久化/研判/gap 关闭）逐层可观测；UI 分列展示 |
| **嵌套去重** | 运行时祖先覆盖检测（祖先已监控 → 子目录跳过 + UI 标注） |
| **取消** | cancel_flag + CancelEvent（SetEvent 从阻塞等待立即退出） |
| **退出钩子** | 应用退出时取消全部监控会话 + 扫描 + 差分 |

## 5. 剩余功能完成情况

| 功能 | 状态 | 说明 |
|---|---|---|
| 来源确认/修正/撤销 | ✅ 完整 | AI 工具页路径行「来源」面板 |
| 历史追踪与检索 | ✅ 基本 | 事件时间线 + 台账状态（分页/高级搜索留后续） |
| 监控授权与隐私设置 | ✅ 完整 | 设置页授权卡 + 总览/历史页状态 |
| 文件身份采集 | ✅ 完整 | 设置页授权 + 总览页运行 + 来源面板展示 |
| 磁盘地图 | ⏳ 占位 | 需要可视化引擎，非核心追踪功能 |
| 文件浏览 | ⏳ 占位 | 需要文件树 UI，非核心追踪功能 |
| AI 工具行为关联 | 📋 M3-C | 规划完成（时间关联候选，不产 AI_CREATED） |
| 浏览器下载记录 | 📋 M3-D | 规划完成（依赖用户逐项授权） |
| USN Journal | 📋 M3-E | 规划完成（需管理员评估） |
| NSIS 安装包 | NOT RUN | tauri build 需网络下载 NSIS 工具链 |

## 6. 数据库迁移与兼容

| 版本 | 内容 | 真实库状态 |
|---|---|---|
| v1 | 基线 11 表 + FTS5 | 已迁移 |
| v2-v4 | 补列（instance_id/source_behavior 等） | 已迁移 |
| v5 | M3-A 五表 + volume_guid + entity_id | 已迁移（真机验收） |
| v6 | M3-B1 台账 + file_events 重建 + scope/coverage 列 | **已迁移**（应用自身执行，508,032 paths 零损失） |

**兼容性**：M3-A 身份/来源/归属数据全部保留；身份撤销只删身份域、保留观察域。

## 7. 自动化测试结果

| 套件 | 结果 |
|---|---|
| `cargo test --workspace` | ✅ **93/93**（core 25 · database 56 · platform 12） |
| `cargo clippy --workspace` | ✅ 0 error / 1 minor style（Ok+? 冗余） |
| `cargo fmt --check` | ✅ |
| `npm test` | ✅ **11/11** |
| `npm run build` | ✅ |
| `node tests/check-readonly.mjs` | ✅ |
| `node tests/validate-schema.mjs` | ✅ 23 表 |
| B2 watcher 集成测试 | ✅ 12/12（监控真机采集/取消/重命名/NTFS 合并容忍） |

## 8. 真实环境测试结果

| 验收项 | 结果 |
|---|---|
| 真实库 v5→v6 迁移（应用自身） | ✅ 508,032 paths 零损失 |
| 真实 E 盘扫描（368K 路径/次 × 6 次） | ✅ done 正常 |
| 自动差分（E 盘真实变化） | ✅ 227 事件（四类齐全） |
| 端到端 DISCOVERED/MODIFIED/MISSING/REAPPEARED | ✅ 全部验证 |
| 历史页 UI 与数据库一致性 | ✅ 逐字段 |
| 撤销身份采集→观察域保留 | ✅ |
| 退出钩子（stderr 实录） | ✅ |
| 身份采集默认关闭 | ✅ 三重验证 |
| RDCW 监控（真机集成测试） | ✅ 12/12 |

## 9. 发布构建

- `cargo build --release -p tracenest-desktop --features custom-protocol` ✅（内嵌前端资产，不依赖 dev server）
- NSIS 安装包：NOT RUN（tauri build 需下载 NSIS 工具链，留后续）

## 10. UI 截图

真机验收截图已从 Git 移除（含本地路径），本地留存于 `docs/evidence/`。
发布可使用虚构测试数据重新截图。

## 11. GitHub 仓库

**https://github.com/Yangxbao/FMap** · Public · main · v0.1.0 已打标

## 12. 上传前安全检查

| 检查项 | 结果 |
|---|---|
| SQLite 数据库文件（.db/.wal/.shm） | ✅ 不在跟踪中（.gitignore 排除） |
| 真机验收截图（含本地路径） | ✅ 已从跟踪移除（.gitignore 排除 docs/evidence/） |
| API Key / Token / 密码 | ✅ 无（零网络出站，无密钥） |
| .env / 个人配置 | ✅ 无 |
| 测试日志 | ✅ .gitignore 排除 *.log |
| Git 历史中的截图 | ⚠️ 存在（已从当前树移除；如需彻底清理可用 BFG repo-cleaner，属可选后续操作） |

## 13. 尚未完成的功能与原因

| 功能 | 原因 |
|---|---|
| M3-C（AI 工具行为关联） | 设计完成；需真实用户使用数据验证；非核心追踪功能 |
| M3-D（浏览器下载记录） | 设计完成；依赖用户逐项授权浏览器数据访问 |
| M3-E（USN Journal） | 设计完成；需管理员权限评估 |
| NSIS 安装包 | tauri build 需下载 NSIS 工具链（网络/构建环境限制） |
| 磁盘地图/文件浏览页 | 需要可视化引擎；非核心追踪功能 |
| macOS/Linux | 代码边界已清晰（core 纯逻辑 / platform 抽象）；需平台测试 |
| 多根扫描拆分独立扫描代 | 已实现（v6 修复）；历史混挂数据需 C 盘重扫覆盖 |

## 14. 需要用户后续人工操作的事项

1. **GitHub 仓库设置**：可选添加 Topics（windows/rust/sqlite/file-monitor）、Discussions 开启
2. **NSIS 安装包**：在有网络的构建环境运行 `cargo tauri build` 生成安装包
3. **Git 历史截图清理**（可选）：`git filter-repo --path docs/evidence --invert-paths` + force push
4. **版本发布页**：在 GitHub Releases 页面上传 NSIS 安装包 + Release Notes

---

*本报告全部数据来自真实执行的测试与验收。NOT RUN 项已标注。无虚构、无降标。*
