# TraceNest 开发计划（DEVELOPMENT_PLAN）

> 版本：v0.9（夜间研究稿） · 日期：2026-11-03
> 前置：开工前需用户确认 NEXT_STEPS_FOR_HUMAN.md 中的 4 项决策

---

## 1. 工程约定

- **语言/UI**：Rust（core）+ TypeScript strict（前端）；中文为第一语言（i18n 架构从第一天建立）。
- **仓库**：单仓库（monorepo 单包）；`main` 分支 + PR；Conventional Commits。
- **测试**：Rust 侧单元测试（scanner/rules/attributor 必测）+ 集成测试（合成文件树 fixture）；前端组件测试轻量为主；CI（GitHub Actions）：Windows runner 跑 `cargo test` + `tsc` + `npm audit` + 安全 grep 门禁。
- **质量门禁**：`cargo clippy -D warnings`；`cargo audit`；无 `unwrap()` 于请求处理路径（错误显式传播到 UI）。

## 2. 里程碑拆解（V0.1 = M0–M5）

### M0 环境与骨架（0.5 周）
- [ ] 安装 Rust 工具链（rustup stable-msvc）+ VS Build Tools（用户已确认 winget 可用；官方链接见 ENVIRONMENT_CHECK.md）
- [ ] `cargo create-tauri-app`（Tauri 2 + React + TS）骨架跑通 `tauri dev`
- [ ] CSP/能力最小化配置；目录结构按 SYSTEM_DESIGN §6

### M1 扫描引擎（1.5 周）
- [ ] 多线程目录遍历（可中断、错误计数、reparse 不跟随、网络盘跳过）
- [ ] 批量写 SQLite（WAL、事务、paths 表 + size 自底向上聚合）
- [ ] 合成文件树 fixture（脚本生成 10 万/100 万文件）+ 基准测试

### M2 规则与归属（1 周）
- [ ] 规则包 JSON schema + serde 校验 + 内置包 v1（来自 AI_TOOLS_DIRECTORIES.md）
- [ ] 目录规则匹配（路径模板展开）+ 信号文件检测 + 置信度评分
- [ ] attributions / agg_tool_size 生成

### M3 查询层与命令（0.5 周）
- [ ] query_tree / query_files / query_file_detail / query_tools / query_projects / query_search（FTS5）
- [ ] 扫描进度事件推送

### M4 前端核心页面（2 周）
- [ ] 布局与主题（对照 prototype/）
- [ ] 首页总览、文件浏览器（虚拟滚动）、AI 识别中心、项目资产页、来源详情、设置页
- [ ] treemap（自研布局 + 下钻；canvas 兜底开关）

### M5 监控与打磨（1 周）
- [ ] notify watcher + 去抖 + 事件流页面 + overflow 计数
- [ ] 提权自检、只读 grep 门禁、日志脱敏
- [ ] 验收清单全项执行（ACCEPTANCE_CRITERIA.md）→ 打 V0.1.0-alpha tag

**V0.1 总计 ≈ 6.5 周**（单人、非全职可 ×1.5–2）

## 3. V0.2 拆解（概要）

1. 项目指纹（F4）：特征文件白名单 + 哈希 + projects 表 + 授权 UX
2. 会话映射（F5）：Claude Code / Codex 适配器先行
3. 去重计量（F6）+ 快照差分（F7）+ 时间线
4. 解释器（F8）/ 卸载清单（F9）/ 大文件哨兵（F14）/ 报告导出（FR-8）

## 4. 风险与应对

| 风险 | 触发信号 | 应对 |
|---|---|---|
| 扫描性能不达标 | M1 基准 >5min/100万 | jwalk/journalnode 替代遍历、增目录 mtime 短路 |
| treemap 渲染卡顿 | M4 >50ms/帧 | 可见节点上限下调、canvas 模式 |
| 规则误报 | 用户反馈/内测 | 置信度分级已有兜底；规则包版本回滚 |
| USN per-file 在无管理员下不可用 | M2 技术验证失败 | 降级：仅用时间戳+目录继承（已在架构上解耦） |
| 单人节奏 | 周期滑动 | 范围裁剪优先级：P0 内再分"必须有/可延" |

## 5. 明日开工建议（待用户确认后执行）

1. 确认 4 项决策（NEXT_STEPS_FOR_HUMAN.md）→ 建正式仓库（用户操作）
2. M0 环境安装（约 30–60 分钟， winget 三条命令）
3. 按里程碑推进；每周向用户同步一次
