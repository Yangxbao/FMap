# AI 编码工具磁盘数据目录调研（原始笔记）

- 用途：TraceNest 桌面应用「AI 工具文件地图」功能的数据源调研
- 调研日期：2026-11-03
- 调研环境：Windows 11 (10.0.26200 x64)，用户 `C:\Users\MR`，Git Bash
- 状态标签定义：
  - **[本机已验证]** — 在本机用 `ls -d` / 顶层 `ls` 核实过存在性（仅存在性 + 顶层条目名，未读取任何文件内容）
  - **[官方文档]** — 出自官方文档站 / 官方 GitHub 仓库文档（含本机随附的官方内置指南）
  - **[社区报告]** — 论坛、博客、第三方聚合文档、GitHub issue 等，非官方但可交叉印证
  - **[待验证]** — 无法可靠确定，标明原因；绝不臆造路径
- 负面结果也计入 [本机已验证]（如"本机不存在"是有价值的观察）
- 本机未安装的工具，其"本机不存在"仅说明该机器状态，不代表路径错误

---

## 1. OpenAI Codex CLI

官方源：repo `openai/codex`（docs/config.md、docs/getting-started.md 等）；官方文档站 developers.openai.com/codex/（含 config-basic / config-advanced / config-reference 三级页面，本环境抓取被 403 拒绝，内容经第三方镜像与社区多源交叉确认）。

### 路径表

| 目录类型 | Windows | macOS | Linux | 状态 | 来源 URL | 内容类型 / 大小特征 / 删除风险 |
|---|---|---|---|---|---|---|
| 配置根目录（CODEX_HOME） | `%USERPROFILE%\.codex` | `~/.codex` | `~/.codex` | [社区报告]（多源一致）+ [本机已验证·存在] | https://github.com/openai/codex/issues/39297 、https://agentgrep.org/backends/codex | 全部运行时状态根；删=丢登录与历史 |
| 配置目录 | `$CODEX_HOME/config.toml`（默认 `~/.codex/config.toml`） | 同左 | 同左 | [官方文档]（repo docs/config.md 提及 config.toml/requirements.toml）+ [本机已验证·存在] | https://github.com/openai/codex/blob/main/docs/config.md | TOML 配置，小文件；删=丢设置 |
| 凭据 | `$CODEX_HOME/auth.json`（默认 `~/.codex/auth.json`） | 同左 | 同左 | [本机已验证·存在]（社区多源一致） | https://raw.githubusercontent.com/openai/codex/main/docs/authentication.md（抓取失败，社区确认） | 登录凭据，小；**勿删** |
| 会话目录 | `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-YYYY-MM-DDThh-mm-ss-<uuid>.jsonl` | 同左 | 同左 | [社区报告]（多源一致：agentgrep、agent-sessions、hivra.cloud、官方 issue #25987/#39297） | https://agentgrep.org/backends/codex 、https://github.com/openai/codex/issues/39297 | 每会话一个 JSONL（rollout），单文件可到 MB 级、总量大；删=无法 resume |
| 历史目录 | `~/.codex/history.jsonl`（输入历史） | 同左 | 同左 | [社区报告]（多源一致；**本机不存在**） | https://jazzyalex.github.io/agent-sessions/blog/where-agents-store-history | 提示词历史 JSONL，中；删=丢输入历史 |
| 日志目录 | `$CODEX_HOME/log`（config.toml `log_dir` 可改，默认 `$CODEX_HOME/log`） | 同左 | 同左 | [社区报告]（官方 config-reference 页存在但 403） | https://www.developersdigest.tech/reference/codex/settings | 日志，中-大；删=安全 |
| 归档会话 | `~/.codex/archived_sessions/` | 同左 | 同左 | [社区报告]（**本机不存在**） | 社区整理文档 | 归档 JSONL；删=丢归档 |
| 临时目录 | `$CODEX_HOME/tmp` | 同左 | 同左 | [本机已验证·存在] | 本机 `ls ~/.codex` | 临时文件；删=安全（运行中除外） |
| 插件/技能 | `$CODEX_HOME/plugins/`、`$CODEX_HOME/skills/`、`$CODEX_HOME/vendor_imports/` | 同左 | 同左 | [本机已验证·存在] | 本机 `ls ~/.codex` | 插件缓存；删=重新下载 |
| 运行时数据库 | `state_5.sqlite`、`logs_2.sqlite`、`memories_1.sqlite`、`queue_1.sqlite`、`goals_1.sqlite`（各带 -shm/-wal）+ `sqlite/` 子目录 | 同左 | 同左 | [本机已验证·存在]（官方文档未逐项说明，用途 [待验证]） | 本机 `ls ~/.codex` | **可能较大**；删=丢对应状态（高危） |
| 安装目录 | npm 全局：`%APPDATA%\npm\node_modules\@openai\codex` | `~/.npm`（nvm 等随 Node 管理器） | 同 macOS | [社区报告]（npm 安装惯例） | https://developers.openai.com/codex/cli（403 未能直接取证） | 程序本体；删=重装 |
| 下载目录 | — | — | — | [待验证]（官方文档未见记载） | — | — |

### 本机验证结果（`ls ~/.codex` 顶层条目名，仅名字）
`auth.json, config.toml, goals_1.sqlite(+shm/wal), installation_id, logs_2.sqlite(+shm/wal), memories_1.sqlite(+shm/wal), plugins, queue_1.sqlite(+shm/wal), skills, sqlite, state_5.sqlite(+shm/wal), tmp, vendor_imports`
- `sessions/`、`log/`、`history.jsonl`、`archived_sessions/` 在本机均**不存在**（`ls -d` 核实）。

### 信息缺口
1. 本机无 `sessions/` 却有 5 组 SQLite 库 + `sqlite/` 目录 → 新版本可能已把会话/状态迁入 SQLite，未找到官方说明（官方 config-reference 403）。**TraceNest 需对"rollout JSONL"与"SQLite"两种形态都做探测。**
2. 官方文档站对爬虫 403，`CODEX_HOME` 默认值与各子路径只能靠社区多源 + 官方 GitHub issue 交叉印证。
3. `downloads` 类目录无官方记载。

---

## 2. Claude Code (Anthropic)

官方源：code.claude.com/docs/en/claude-directory（2026-10-29 版，本调研完整抓取）；辅以 /docs/en/settings、/docs/en/env-vars。

### 核心结论：会话 ↔ 项目工作目录映射（TraceNest 最关键）
- 会话转录：`~/.claude/projects/<project>/<session>.jsonl`，一行一条消息的明文 JSONL，含工具调用、工具结果、系统提示、工具定义、请求参数（明文，未加密）。[官方文档]
- `<project>`（项目 slug）编码规则：**工作目录路径中除字母和数字外的每个字符都替换为 `-`**。官方原文（scratchpad 一节）："`<project>` is your working directory path with every character other than letters and digits replaced by `-`, such as `-Users-you-my-project`"。[官方文档]
- 官方 `claude purge` 示例同时印证 projects/ 目录同名规则：`/home/user/work/my-repo` → `~/.claude/projects/-home-user-work-my-repo`。[官方文档]
- 推导（TraceNest 需自证）：Windows `D:\.zcode\workspace\X` → 驱动器冒号、反斜杠全部变 `-`，形如 `D--zcode-workspace-X`。此为按官方规则的推论 [社区报告/推断]，本机 `~/.claude/projects` 不存在，无法本地验证。
- 同目录还有：`<session>.orphaned-<时间戳>-<后缀>.jsonl`、`<session>.jsonl.superseded-<时间戳>`（被搁置的旧转录）；`<session>/subagents/`（子代理转录）；`<session>/tool-results/`（大工具输出溢出文件 + MCP 图片）；`<session>/mcp-tasks/`。[官方文档]
- 保留策略：`cleanupPeriodDays` 默认 30 天，超期自动清扫（文件级）。[官方文档]

### 路径表

| 目录类型 | Windows | macOS | Linux | 状态 | 来源 URL | 内容类型 / 大小特征 / 删除风险 |
|---|---|---|---|---|---|---|
| 配置目录 | `%USERPROFILE%\.claude` | `~/.claude` | `~/.claude` | [官方文档] + [本机已验证·存在] | https://code.claude.com/docs/en/claude-directory | 根目录；删=丢全部配置/历史 |
| 配置目录重定向 | 环境变量 `CLAUDE_CONFIG_DIR` 指向的目录 | 同左 | 同左 | [官方文档] | 同上 | 设了它则本表所有 `~/.claude` 路径都挪过去 |
| 全局状态文件 | `%USERPROFILE%\.claude.json` | `~/.claude.json` | `~/.claude.json` | [官方文档] + [本机已验证·不存在] | 同上 | 账号/项目信任/个人 MCP/UI 状态，小；**勿删** |
| 会话目录（核心） | `~/.claude\projects\<project>\<session>.jsonl` | 同形 | 同形 | [官方文档] + [本机已验证·projects 不存在] | 同上 | 明文 JSONL，**最大文件源**；删=丢 resume/rewind |
| 检查点快照 | `~/.claude\file-history\<session>\` | 同形 | 同形 | [官方文档] | 同上 | 编辑前文件快照（近 100 个 checkpoint）；可大；删=丢回滚 |
| 计划 | `~/.claude\plans\` | 同形 | 同形 | [官方文档] | 同上 | plan 模式产物，小 |
| 调试日志 | `~/.claude\debug\` | 同形 | 同形 | [官方文档] | 同上 | `--debug` 会话日志，中-大；删=安全 |
| 粘贴缓存 | `~/.claude\paste-cache\` | 同形 | 同形 | [官方文档] | 同上 | 大粘贴内容；删=丢历史粘贴 |
| 图片缓存 | `~/.claude\image-cache\<session>\`（≤v2.1.274） | 同形 | 同形 | [官方文档] | 同上 | 旧版贴图；新版移到临时目录 |
| 上传附件 | `~/.claude\uploads\<session>\` | 同形 | 同形 | [官方文档] | 同上 | Web/移动 Remote Control 附件 |
| 会话环境 | `~/.claude\session-env\`、`~/.claude\dev-mods\<session>\`、`~/.claude\tasks\` | 同形 | 同形 | [官方文档] | 同上 | 各类会话级状态，小 |
| Shell 快照 | `~/.claude\shell-snapshots\` | 同形 | 同形 | [官方文档] + [本机已验证·不存在] | 同上 | Bash 启动快照，干净退出即删 |
| 备份 | `~/.claude\backups\`（.claude.json 旧版，保留 5 份） | 同形 | 同形 | [官方文档] | 同上 | 小-中 |
| 反馈/用量 | `~/.claude\feedback-bundles\`、`~/.claude\feedback\drafts\`、`~/.claude\usage-data\`（/insights report.html） | 同形 | 同形 | [官方文档] | 同上 | 中 |
| 凭据 | `~/.claude\.credentials.json` | 同形 | 同形 | [官方文档] | 同上 | 登录凭据；**勿删** |
| 插件 | `~/.claude\plugins\`（marketplace 克隆、`installed_plugins.json`、`synced\`） | 同形 | 同形 | [官方文档] + [本机已验证·不存在] | 同上 | **可大**（克隆仓库）；删=重装插件 |
| 自动记忆 | `~/.claude\projects\<project>\memory\`（MEMORY.md 等）、`~/.claude\agent-memory\` | 同形 | 同形 | [官方文档] | 同上 | 小；不受 30 天清扫 |
| 常驻状态 | `~/.claude\jobs\`、`~/.claude\daemon\`、`~/.claude\sessions\`（每运行会话一个小文件） | 同形 | 同形 | [官方文档] | 同上 | 小；勿删 |
| 缓存杂项 | `~/.claude\stats-cache.json`、`remote-settings.json`、`cache\changelog.md`、`policy-limits.json` | 同形 | 同形 | [官方文档] | 同上 | 小；删=自动重建 |
| 旧版遗留 | `~/.claude\todos\`、`~/.claude\statsig\`、`~/.claude\logs\` | 同形 | 同形 | [官方文档]（已不再写入，清扫会删） | 同上 | 旧版本遗留 |
| 临时工作区 | `%TEMP%\claude\<project>\<session-id>\scratchpad\` | `/private/tmp/claude-<uid>/...` | `/tmp/claude-<uid>/...`（或 `$TMPDIR`） | [官方文档]（`CLAUDE_CODE_TMPDIR` 可改） | 同上 | 每会话暂存目录；随转录清扫或系统清 temp |
| 项目内配置 | `<repo>\.claude\`（settings.json、skills/、agents/、rules/…）、`CLAUDE.md`、`.mcp.json` | 同形 | 同形 | [官方文档] | 同上 | 属于项目工作目录，非用户盘数据 |
| 安装目录 | npm 全局 `%APPDATA%\npm\node_modules\@anthropic-ai\claude-code`；原生安装器 `~/.local/bin/claude.exe` | 同形 | 同形 | [社区报告]（npm 惯例/发行说明，未直接取证） | https://code.claude.com/docs/en/quickstart | 程序本体 |
| 下载目录 | — | — | — | [待验证]（官方文档未记载 downloads 目录） | — | — |

### 本机验证结果
- `~/.claude` 存在，**顶层仅有 `settings.json`**；`projects/`、`todos/`、`shell-snapshots/`、`plugins/`、`statsig/`、`downloads/` 均不存在；`~/.claude.json` 不存在（本机用 ZCode，未跑过 Claude Code）。

### 信息缺口
1. Windows 下 `<project>` slug 的实际形态（`D:\` → `D--`？盘符大小写？长路径截断？）未经本机验证，需在有真实会话的机器上采样确认。
2. Claude Desktop / Cowork 转录的落盘位置官方未在本页给出（仅说保留策略不同）。
3. 旧版 `todos/`、`statsig/`、`logs/` 只在旧版本存在。

---

## 3. Cursor（VSCode fork 桌面版）

官方源：cursor.com/docs（docs.cursor.com 已 308 跳转）；MCP 配置路径为官方文档确认；AppData 内部结构官方**无完整记载**，依赖 VSCode fork 惯例 + 社区/论坛证据。

### 路径表

| 目录类型 | Windows | macOS | Linux | 状态 | 来源 URL | 内容类型 / 大小特征 / 删除风险 |
|---|---|---|---|---|---|---|
| 全局 MCP 配置 | `%USERPROFILE%\.cursor\mcp.json` | `~/.cursor/mcp.json` | `~/.cursor/mcp.json` | [官方文档]（docs 仅给 `~/.cursor/mcp.json` 写法；Windows 变体为惯例推导） | https://cursor.com/docs/context/mcp | JSON 配置，小；删=丢 MCP 服务器 |
| 项目 MCP 配置 | `<repo>\.cursor\mcp.json` | 同形 | 同形 | [官方文档] | 同上 | 团队共享配置 |
| 配置目录 | `~/.cursor` | 同左 | 同左 | [本机已验证·不存在] | 本机 `ls -d` | 官方文档只证实 mcp.json 在此 |
| 用户数据目录（Chromium/VSCode 式） | `%APPDATA%\Cursor`（含 `Cache`、`Code Cache`、`GPUCache`、`Service Worker`、`User\globalStorage`、`User\workspaceStorage`） | `~/Library/Application Support/Cursor` | `~/.config/Cursor` | [社区报告]（VSCode fork 惯例 + 论坛帖证实子目录名） | https://forum.cursor.com/t/151747 、https://vibe-replay.com/blog/cursor-local-storage | **磁盘占用主战场**；见下"大文件" |
| 安装目录 | `%LOCALAPPDATA%\Programs\cursor`（含 Cursor.exe、resources\app） | `/Applications/Cursor.app` | `/usr/share/cursor` 等 | [社区报告]（VSCode 惯例；本机不存在） | 社区帖 | 程序本体 |
| 聊天/Agent 会话 | `%APPDATA%\Cursor\User\globalStorage` 内 `state.vscdb`（`cursorDiskKV` 表）+ `agent-transcripts`；新版另有 `~/.cursor/chats` | 同形 | 同形 | [社区报告]（论坛 + vibe-replay 一致） | https://forum.cursor.com/t/151747 、https://vibe-replay.com/blog/cursor-local-storage | **最大增长点**：论坛报告 state.vscdb 一天涨 1GB、单人 125GB；删=丢聊天历史 |
| 缓存目录 | `%APPDATA%\Cursor\Cache`、`Code Cache`、`CachedData`（陈旧目录无人清理） | `~/Library/Caches/Cursor` | `~/.config/Cursor/CachedData` | [社区报告]（论坛帖：CachedData 无限累积） | https://forum.cursor.com/t/141573 | 大；删=安全（下次重建） |
| 日志目录 | `%APPDATA%\Cursor\logs` | 同形 | 同形 | [社区报告]（VSCode 惯例） | 社区帖 | 中；删=安全 |
| 下载目录 | 用户下载走系统 Download；扩展下载进 `%USERPROFILE%\.cursor\extensions` | `~/.cursor/extensions` | 同 | [社区报告]（VSCode 惯例） | 社区帖 | 扩展可大 |

### 本机验证结果
- `~/.cursor`、`%APPDATA%\Roaming\Cursor`、`%LOCALAPPDATA%\Programs\cursor`、`%LOCALAPPDATA%\cursor` 均不存在（本机未装 Cursor）。

### 信息缺口
1. 官方文档不记载 `%APPDATA%\Cursor` 内部结构；`globalStorage/state.vscdb`、`agent-transcripts`、`~/.cursor/chats` 的准确边界与版本差异需实机采样。
2. Cursor 聊天历史在 `state.vscdb (cursorDiskKV)`、`~/.cursor/chats`、`agent-transcripts` 之间的版本迁移关系未定论。
3. 官方无自动清理机制（论坛多帖证实），TraceNest 清理功能需谨慎：`state.vscdb` 同时存设置+历史。

---

## 4. Gemini CLI

官方源：repo `google-gemini/gemini-cli` docs/cli/settings.md、docs/cli/checkpointing.md（本调研完整抓取）。

### 路径表

| 目录类型 | Windows | macOS | Linux | 状态 | 来源 URL | 内容类型 / 大小特征 / 删除风险 |
|---|---|---|---|---|---|---|
| 配置目录 | `%USERPROFILE%\.gemini` | `~/.gemini` | `~/.gemini` | [官方文档]（settings.md 载明 `~/.gemini/settings.json`）+ [本机已验证·不存在] | https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/settings.md | 根目录 |
| 配置文件（用户） | `~/.gemini\settings.json` | 同左 | 同左 | [官方文档] | 同上 | JSON 设置，小；**勿删** |
| 配置文件（项目） | `<project>\.gemini\settings.json`（workspace 覆盖用户） | 同形 | 同形 | [官方文档] | 同上 | 项目级设置 |
| 检查点（Git 快照） | `~/.gemini\history\<project_hash>\`（shadow git 仓库） | 同左 | 同左 | [官方文档] | https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/checkpointing.md | **可大**（整仓库快照）；删=丢 /restore |
| 检查点（会话 JSON） | `~/.gemini\tmp\<project_hash>\checkpoints\` | 同左 | 同左 | [官方文档] | 同上 | 对话与工具调用 JSON；删=丢恢复点 |
| 旧版检查点位置 | `/tmp/gemini/<project_hash>`（旧文档/社区记忆） | 同 | 同 | [社区报告]（现行官方文档已改为 `~/.gemini/tmp/...`；Reddit 报告现落 `~/.gemini/tmp/`） | https://www.reddit.com/r/GeminiAI/comments/1qrhmtk/ | 仅旧版本 |
| 凭据 | `~/.gemini\oauth_creds.json` | 同左 | 同左 | [社区报告] + [官方仓库 issue #28439 提及] | https://github.com/google-gemini/gemini-cli/issues/28439 | OAuth 令牌；**勿删** |
| 计划产物 | `general.plan.directory`（未设则系统临时目录） | 同 | 同 | [官方文档]（settings.md） | 同 settings.md | 小 |
| 自动记忆 | `<projectMemoryDir>\.inbox\<kind>\*.patch` | 同 | 同 | [官方文档]（settings.md auto memory） | 同上 | 小 diff 文件 |
| 遥测 | 通过 `settings.json` `telemetry` 配置 OTLP 上报，本地是否落盘 | — | — | [待验证]（docs/cli/telemetry.md 存在未详读） | https://github.com/google-gemini/gemini-cli/tree/main/docs/cli | — |
| 会话目录 | `~/.gemini/tmp/` 下按会话存临时文件（Reddit 报告每会话随机文件） | 同 | 同 | [社区报告]（与官方 checkpointing 路径一致） | Reddit 帖 + issue #7291 | 中 |
| 安装目录 | npm 全局 `@google/gemini-cli` | 同 | 同 | [社区报告]（npm 惯例） | repo README | 程序本体 |

### 本机验证结果
- `~/.gemini` 不存在（本机未装）。

### 信息缺口
1. `<project_hash>` 的哈希算法官方未说明（checkpointing.md 只给相对路径），TraceNest 反向映射"项目目录 → 检查点"需破解该哈希或采样统计。
2. 缓存/遥测本地落盘目录未见官方记载。

---

## 5. OpenCode

官方源：opencode.ai/docs/config/（本调研抓取）；存储路径官方 config 页未记载，靠社区多源。

### 路径表

| 目录类型 | Windows | macOS | Linux | 状态 | 来源 URL | 内容类型 / 大小特征 / 删除风险 |
|---|---|---|---|---|---|---|
| 配置文件（全局） | `%USERPROFILE%\.config\opencode\opencode.json`（TUI 另有 `tui.json`） | `~/.config/opencode/opencode.json` | 同 | [官方文档] | https://opencode.ai/docs/config/ | JSON 配置；**勿删** |
| 配置文件（项目） | `<repo>\opencode.json` 或 `.opencode\`（向上搜索到最近 git 根） | 同形 | 同形 | [官方文档] | 同上 | 项目配置 |
| 配置环境变量 | `OPENCODE_CONFIG`、`OPENCODE_CONFIG_DIR`、`OPENCODE_TUI_CONFIG`、`OPENCODE_CONFIG_CONTENT` | 同 | 同 | [官方文档] | 同上 | 可整体搬移 |
| 托管配置 | `%ProgramData%\opencode` | `/Library/Application Support/opencode/` | `/etc/opencode/` | [官方文档] | 同上 | 企业管理配置 |
| 资源目录 | `~/.config/opencode` 下 `agents/ commands/ plugins/ skills/ tools/ themes/`（复数名；单数向后兼容） | 同 | 同 | [官方文档] | 同上 | 用户扩展资源 |
| 数据/会话目录 | `%LOCALAPPDATA%` 或 `~/.local/share/opencode`（**Windows 路径未定论**） | `~/.local/share/opencode` | `~/.local/share/opencode` | [社区报告]（Linux/mac 多源一致）+ [待验证]（Windows） | https://vibe-replay.com/blog/opencode-local-storage 、https://docs.rs/opencode-session-restore | 见下两行 |
| 会话存储（现行） | `~/.local/share/opencode/opencode.db`（SQLite：sessions/messages/parts/todos/shares/permissions/projects/credentials/events 表） | 同 | 同 | [社区报告]（多源一致） | 同上 | **主要大文件**；删=丢全部会话 |
| 会话存储（旧版） | `~/.local/share/opencode/storage\`：`session/<projectID>/<sessionID>.json`、`message/<sessionID>/<messageID>.json`、`part/<messageID>/*.json` | 同 | 同 | [社区报告]（多源一致） | https://forums.basehub.com/anomalyco/opencode/15 | 旧版 JSON；删=丢旧会话 |
| 凭据 | `~/.local/share/opencode\auth.json` | 同 | 同 | [社区报告] | 同上 | API key；**勿删** |
| 缓存目录 | `~/.cache/opencode`（推断，遵循 XDG） | 同 | 同 | [待验证]（官方未见记载） | — | — |
| 安装目录 | npm 全局 `opencode-ai` 或独立安装器 | 同 | 同 | [社区报告]（惯例） | repo `sst/opencode` | 程序本体 |

### 本机验证结果
- `~/.config/opencode`、`~/.local/share/opencode`、`~/.local/state/opencode` 均不存在（本机未装）。

### 信息缺口
1. Windows 下数据目录到底映射到哪（XDG 库对 Windows 的处理：`%LOCALAPPDATA%` 还是 `~/.local/share`）— [待验证]，需实机安装采样。
2. `opencode.db` 的表结构与可安全清理范围官方未记载。

---

## 6. ZCode CLI（本机工具）

官方公开文档：未检索到公开官方文档站；仅社区文章（Medium、4sapi、developersdigest 等）提及 `~/.zcode`。**工具自带官方指南**（本机随附 zcode-configuration-guide 插件）记载了配置路径，可信度按 [官方文档·内置指南] 处理。

### 路径表

| 目录类型 | Windows | macOS | Linux | 状态 | 来源 URL | 内容类型 / 大小特征 / 删除风险 |
|---|---|---|---|---|---|---|
| 配置根目录 | `%USERPROFILE%\.zcode` | `~/.zcode` | `~/.zcode` | [本机已验证·存在] | 本机 `ls ~/.zcode` | 根目录；勿整体删 |
| 用户配置文件 | `~/.zcode\cli\config.json`（MCP `mcp.servers`、hooks、plugins 启停） | 同左 | 同左 | [官方文档·内置指南] + [本机已验证·存在] | 本机随附 zcode-configuration-guide SKILL.md | JSON 配置；**勿删** |
| 用户指令 | `~/.zcode\AGENTS.md` | 同左 | 同左 | [官方文档·内置指南] | 同上 | 小 |
| 技能/命令 | `~/.zcode\skills\`、`~/.zcode\commands\`（fallback `~/.agents\skills`、`~/.agents\commands`） | 同 | 同 | [官方文档·内置指南] + [本机已验证·skills 存在] | 同上 | 小 |
| 工作区配置 | `<repo>\.zcode\config.json` 或 `<repo>\zcode.json`、`<repo>\AGENTS.md` | 同 | 同 | [官方文档·内置指南] | 同上 | 属于项目目录 |
| CLI 运行时 | `~/.zcode\cli\`（agents, artifacts, db, exec, image-cache, log, memories, node-fallback, pdf-cache, plugins, rollout, config.json） | 同 | 同 | [本机已验证·顶层名] | 本机 `ls ~/.zcode/cli` | `db/`、`rollout/`、`image-cache/`、`pdf-cache/` **可能大**；log/ 可清 |
| v2 应用状态 | `~/.zcode\v2\`（bot-config.v3.json, bot-state.v3.json, credentials.json, setting.json） | 同 | 同 | [本机已验证·顶层名]（用途 [待验证]） | 本机 `ls ~/.zcode/v2` | 凭据在 credentials.json；**勿删** |
| 其他 | `~/.zcode\plugin-workspace\`、`~/.zcode\workspace\`、`~/.zcode\tmp\` | 同 | 同 | [本机已验证·顶层名] | 本机 `ls ~/.zcode` | workspace/tmp 可大；tmp 可清 |
| 插件缓存 | `~/.zcode\cli\plugins\`（含 `cache\zcode-plugins-official\...`） | 同 | 同 | [本机已验证·存在] | 本机观察 | 可大；删=重新拉取 |
| 会话目录 | `~/.zcode\cli\rollout\`（命名与 Claude/Codex 的 rollout 惯例一致，具体格式未记载） | 同 | 同 | [本机已验证·存在] + [待验证]（格式与编码规则无公开文档） | 本机观察 | 会话数据；删=丢恢复 |
| 日志目录 | `~/.zcode\cli\log\` | 同 | 同 | [本机已验证·存在] | 本机观察 | 删=安全 |
| 安装目录 | （随客户端安装，本机 `E:\Develop\ZCode\` 下含 resources） | — | — | [本机已验证·存在]（仅本机观察） | 本机环境 | 程序本体 |

### 信息缺口
1. 无公开官方文档：`cli/db`、`cli/rollout` 的会话文件格式、`<project>` 编码规则均 [待验证·仅本机观察]。
2. `v2/` 各 JSON 的用途未记载（credentials.json 含凭据，禁止读取）。

---

## 7. WorkBuddy（= 腾讯 CodeBuddy Code 的品牌名，workbuddy.ai）

身份确认：workbuddy.ai 为腾讯 CodeBuddy 相关产品站（CLI 版发布说明至 v2.177.1）；docs 站为 JS 渲染，本次抓取只取到标题，无法引述内容。[官方文档存在但未获取]

### 路径表

| 目录类型 | Windows | macOS | Linux | 状态 | 来源 URL | 备注 |
|---|---|---|---|---|---|---|
| 配置根目录 | `%USERPROFILE%\.codebuddy`（`CODEBUDDY_HOME` / `--codebuddy-home` 可覆盖；"Windows defaults under `%USERPROFILE%`"） | `~/.codebuddy` | `~/.codebuddy` | [社区报告]（第三方工具 memorax-code 的 configuration.md、dsh-session-import 插件页一致引用） | https://github.com/memorax-ai/memorax-code/blob/main/docs/configuration.md 、https://dsh-plugin.market/plugin/devmom/dsh-session-import | 会话导入工具按此目录读取 |
| 会话文件格式 | OpenAI 消息格式 / 通用 JSON 导出 | 同 | 同 | [社区报告] | dsh-plugin.market 插件页 | 具体子目录 [待验证] |
| 安装/缓存/日志 | — | — | — | [待验证]（官方 docs 未能抓取，未找到可靠社区路径） | https://www.workbuddy.ai/docs | 需实机安装或抓取 docs/CLI 子页 |

### 信息缺口
- 官方文档站可访问性差（SPA），`~/.codebuddy` 内部布局（会话、缓存、日志）全部 [待验证]。理由：官方内容未获取，社区引用为第三方工具作者的描述，未经第二官方源印证。

---

## 8. 豆包 Doubao 桌面版（字节跳动）

官方文档：无面向开发者的目录文档（预期内）。仅有社区文章。

### 调查结果

| 目录类型 | Windows | macOS | Linux | 状态 | 来源 URL | 备注 |
|---|---|---|---|---|---|---|
| 安装目录 | 疑似 `%LOCALAPPDATA%` 下（未定论） | — | — | [待验证] | — | 搜索未给出确切路径 |
| 用户数据/缓存 | 疑似 `%APPDATA%\Doubao` 或 `%LOCALAPPDATA%\Doubao`（Electron 惯例候选）；内有 Cache、GPUCache、Service Worker\CacheStorage（若 Electron） | — | — | [社区报告·弱] + [待验证] | https://blog.csdn.net/weixin_29315569/article/details/162648952 | **关键社区报告：CSDN 文章称"豆包电脑版不是安装包，而是 PWA 网页应用"**，若属实则数据在浏览器配置档内，而非独立 AppData 目录 |
| 生成内容保存 | 默认走系统下载目录（网页版） | 同 | — | [社区报告] | https://blog.csdn.net/weixin_28725949/article/details/166025103 | CSDN 实操指南 |
| 可靠性评级 | **低** — 两条社区线索互相矛盾（PWA vs 独立客户端），无官方说明 | — | — | [待验证] | — | TraceNest 需实机采样 |

### 信息缺口
- 连"是否存在独立桌面客户端及其打包方式"都未定论；全部目录 [待验证]。理由：无官方文档，社区来源少且互相矛盾。

---

## 9. DSH

身份确认：**DSH = DeepSeek Harness**，官方仓库 `deepseek-ai/deepseek-harness` 真实存在（"Everything is a Plugin"、基于 Cordis、TypeScript/Node、MIT；`npx @deepseek-ai/dsh web` 启动 Web UI 于 http://127.0.0.1:3080）。README 未提及任何配置/会话路径。[官方仓库确认]（本调研直接抓取仓库页）

### 调查结果

| 目录类型 | Windows | macOS | Linux | 状态 | 来源 URL | 备注 |
|---|---|---|---|---|---|---|
| 配置根目录 | `%USERPROFILE%\.dsh`（`DSH_HOME` 覆盖，"root directory for DeepSeek Harness configuration, sessions, and plugin data"，profiles 在 `$DSH_HOME/profiles/<name>`） | `~/.dsh` | `~/.dsh` | [社区报告·低可信]（内容农场博客 ofox.ai、dsh-plugin.org；官方 README 无此记载） | https://ofox.ai/blog/deepseek-harness-dsh-setup-custom-providers-2026 、https://dsh-plugin.org/tutorials/dsh-data-layout | 需以官方仓库源码/文档核实 |
| 会话目录 | "会话绑定工作区目录"，存储是插件能力（packages/storage/storage-domain/） | 同 | 同 | [社区报告·低可信] | https://blog.yeyupiaoling.cn/article/1791865095798 | 无官方布局说明 |
| 其余（安装/缓存/日志） | — | — | — | [待验证] | https://github.com/deepseek-ai/deepseek-harness | 官方 README 只写安装与 Web UI |

### 信息缺口
- 全部具体路径 [待验证]。理由：官方 README/docs 未列磁盘布局，第三方来源均为内容农场级，交叉性不足；下一步应读官方仓库源码（Cordis storage 域）确定。

---

## 10. Bonus：AI 工具常见的共享大缓存

| 缓存 | Windows | macOS | Linux | 状态 | 来源 | 大小特征 |
|---|---|---|---|---|---|---|
| HuggingFace 模型缓存 | `%USERPROFILE%\.cache\huggingface\hub\models--<org>--<name>\blobs` | `~/.cache/huggingface/...` | `~/.cache/huggingface/...` | [官方文档] | https://huggingface.co/docs/huggingface_hub/guides/manage-cache | **最大可达数十-数百 GB**；删=重新下载 |
| Ollama 模型 | `%USERPROFILE%\.ollama\models` | `~/.ollama/models` | `~/.ollama/models` | [官方文档] | https://github.com/ollama/ollama/blob/main/docs/faq.md | 每 GB 级模型；删=重新拉取 |
| pip 缓存 | `%LocalAppData%\pip\Cache` | `~/Library/Caches/pip` | `~/.cache/pip` | [官方文档] | https://pip.pypa.io/en/stable/cli/pip_cache/ | 中-大；删=安全 |
| npm 缓存 | `%LocalAppData%\npm-cache`（`_cacache`） | `~/.npm/_cacache` | `~/.npm/_cacache` | [官方文档] | https://docs.npmjs.com/cli/v11/configuring-npm/folders | 大；`npm cache clean --force` |
| pnpm store | `%LOCALAPPDATA%\pnpm\store` | `~/Library/pnpm/store` | `~/.local/share/pnpm/store`（版本相关；v3 曾为 `~/.pnpm-store`） | [官方文档] | https://pnpm.io/cli/store | 大；`pnpm store prune` |
| Cargo（Rust） | `%USERPROFILE%\.cargo\registry`（CARGO_HOME） | `~/.cargo/registry` | `~/.cargo/registry` | [官方文档] | https://doc.rust-lang.org/cargo/reference/config.html | 大；删=重新编译依赖 |
| uv 缓存 | `%LOCALAPPDATA%\uv\cache` | `~/Library/Caches/uv` | `~/.cache/uv` | [官方文档] | https://docs.astral.sh/uv/concepts/cache | 大；`uv cache clean` |
| Go 模块缓存 | `%USERPROFILE%\go\pkg\mod`（GOMODCACHE） | `$HOME/go/pkg/mod` | `$HOME/go/pkg/mod` | [官方文档] | https://pkg.go.dev/cmd/go#hdr-Module_configuration | 大；`go clean -modcache` |
| Conda | `%USERPROFILE%\miniconda3\envs`（环境）+ `pkgs\`（包缓存） | `~/miniconda3/envs` + `pkgs/` | 同 | [官方文档] | https://docs.anaconda.com/ | 环境可极大；`conda clean` |

---

## 汇总表

| 工具 | 本机已验证 | 官方文档 | 社区报告 | 待验证 | 最大信息缺口 |
|---|---|---|---|---|---|
| Codex CLI | 8（.codex 存在、config.toml、auth.json、tmp/、sqlite 库×5、plugins/skills 等；负面：sessions/log/history 不存在） | 1（config.toml 官方提及） | 6（CODEX_HOME、sessions/rollout、history.jsonl、log、archived_sessions、npm 安装） | 1（本机无 sessions 的现行机制） | 新版会话是否迁入 SQLite；官方 config-reference 被反爬（403） |
| Claude Code | 3（.claude 存在且仅 settings.json；.claude.json/projects 等不存在） | 21（全目录树 + slug 编码规则 + 保留策略） | 2（npm/原生安装路径） | 1（downloads 未记载） | Windows 下 `<project>` slug 实际形态（`D--zcode-...`）未实机验证；Desktop/Cowork 转录位置 |
| Cursor | 3（全部为"本机不存在"负面结果） | 2（~/.cursor/mcp.json、.cursor/mcp.json） | 8（AppData 结构、state.vscdb、chats、CachedData、安装目录） | 1（AppData 内部结构官方不记载） | 聊天数据在 state.vscdb / ~/.cursor/chats / agent-transcripts 间的版本边界 |
| Gemini CLI | 1（本机不存在） | 5（settings.json×2、checkpointing×2、plan 目录） | 4（oauth_creds.json、/tmp/gemini 旧版、tmp 会话、npm 安装） | 1（遥测本地落盘） | `<project_hash>` 哈希算法（阻碍 项目↔检查点 反向映射） |
| OpenCode | 3（全部"不存在"） | 6（config×4、托管路径、资源目录） | 4（opencode.db、legacy storage/、auth.json、安装） | 2（Windows 数据目录、cache 目录） | Windows 下 `~/.local/share/opencode` 的实际映射 |
| ZCode CLI | 14（.zcode 三层结构全列出） | 6（内置指南：config.json、AGENTS.md、skills/commands/.agents、workspace 规则） | 1（社区文章提及 ~/.zcode） | 2（rollout/db 格式、v2 用途） | 无公开官方文档：会话文件格式与项目编码规则 |
| WorkBuddy | 0 | 0（docs 站存在但 SPA 无法抓取） | 2（~/.codebuddy、CODEBUDDY_HOME） | 2（安装/缓存/日志、会话内部布局） | 官方文档内容未获取；~/.codebuddy 布局全靠第三方 |
| 豆包 Doubao | 0 | 0 | 2（PWA 说法、缓存清理文章） | 3（安装/数据/缓存全未知） | 是否 PWA vs 独立客户端都未定论，无可靠路径 |
| DSH | 0 | 1（官方仓库存在性确认；README 无路径） | 2（~/.dsh、会话绑定工作区；低可信） | 3（全部磁盘路径） | 官方 README/docs 无布局；需读 Cordis storage 源码 |
| 共享缓存 ×9 | 0 | 9 | 0 | 0 | pnpm store 位置随版本变化需运行时探测 |

合计（近似）：本机已验证 32 条 · 官方文档 51 条 · 社区报告 31 条 · 待验证 16 条。

### 对 TraceNest 的三条落地建议（依据以上证据）
1. **Claude Code 会话映射**是唯一有官方编码规则的映射（非字母数字 → `-`），实现它 + `claude purge --dry-run` 输出可作自校验；Codex rollout 按日期分层、OpenCode 用 SQLite、Gemini 用 project_hash，四家四种形态需各自适配器。
2. **Cursor 是磁盘占用监控第一优先级**（论坛实证 state.vscdb 单表 1GB/天、总占用 125GB+ 案例），但清理必须绕开 `globalStorage/state.vscdb`（设置+历史同库）。
3. **本机无 sessions 目录的 Codex** 提示：目录探测必须做"存在性 + SQLite 存在性"双分支，且结论按机器实际版本校准。
