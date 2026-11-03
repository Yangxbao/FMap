# AI 工具目录研究报告（AI_TOOLS_DIRECTORIES）

> 版本：v1.1 · 日期：2026-11-03
> 研究方法：官方文档/官方仓库优先 → 本机只读验证（Windows 11，仅检查路径存在性与**顶层目录名**，未读取任何文件内容，未触碰会话记录）→ 社区报告交叉核对。
> 状态标签：**[本机已验证]** · **[官方文档]** · **[社区报告]** · **[待验证]**
> 原始调研笔记（逐工具完整路径表）：[notes/ai-tools-directories-raw.md](notes/ai-tools-directories-raw.md)

---

## 0. 本机验证总览（2026-11-03）

| 工具 | 本机存在 | 本机关键发现 |
|---|---|---|
| Codex CLI / Desktop | ✅ `~/.codex` | config.toml、auth.json、**5 组 SQLite 库（state/logs/memories/queue/goals）**、plugins/、skills/、.sandbox 系列；**无 sessions/ 与 history.jsonl → 新版已迁移 SQLite** |
| Claude Code | ✅ `~/.claude` | 本机仅有 `settings.json`（projects/ 等未生成——使用少或新装） |
| ZCode CLI | ✅ `~/.zcode` | `cli/`（内含 agents、db、rollout、image-cache、pdf-cache、log、memories…）、`v2/`、`skills/`、`plugin-workspace/`；工作区 `D:\.zcode\workspace` |
| 豆包（桌面版） | ✅ 双目录 | `%APPDATA%\Doubao\public_config.json`；`%LOCALAPPDATA%\Doubao\Application + User Data`（Electron/Chromium 布局） |
| DSH | ✅ `~/.dsh` | `sessions/ attachments/ profiles/ storages/ dsh-runtimes/ llm-deepseek/ .credentials.yaml` |
| WorkBuddy | ✅ `~/.workbuddy` | `app/ binaries/ blobs/ cache/ artifact-index/ audit-log/ connectors/ changes-index/ cli-env/` + SQLite 迁移标记 + UUID 子目录 |
| Cursor | ❌ 未安装 | `~/.cursor`、`%APPDATA%\Cursor`、`%LOCALAPPDATA%\Programs\cursor` 均不存在 |
| Gemini CLI | ❌ 未安装 | `~/.gemini` 不存在 |
| OpenCode | ❌ 未安装 | XDG 三目录均不存在 |
| 共享缓存 | 部分存在 | pip cache ✅、npm-cache ✅；ollama/HF/pnpm/cargo/uv/go 未出现 |

**身份确认（本次研究的重要成果）**：
- **DSH = DeepSeek Harness**，官方仓库 [deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness) 真实存在（Cordis 插件架构、`npx @deepseek-ai/dsh`、Web UI 127.0.0.1:3080），但 README 未记载磁盘路径 [待验证]。
- **WorkBuddy = 腾讯 CodeBuddy Code 的品牌**（workbuddy.ai 有官方文档站，JS 渲染无法抓取正文）；`~/.codebuddy` + `CODEBUDDY_HOME` 来自第三方工具文档 [社区报告]。

---

## 1. Codex CLI / Desktop（OpenAI）

| 目录类型 | Windows | macOS/Linux | 状态 | 来源 |
|---|---|---|---|---|
| 主目录（CODEX_HOME） | `%USERPROFILE%\.codex` | `~/.codex` | [官方文档]+[本机已验证] | [openai/codex](https://github.com/openai/codex)、[issue #39297](https://github.com/openai/codex/issues/39297) |
| 配置 | `~/.codex\config.toml` | 同 | [官方文档]+[本机已验证] | [docs/config.md](https://github.com/openai/codex/blob/main/docs/config.md) |
| 凭据 | `~/.codex\auth.json` | 同 | [本机已验证] | 本机 |
| **新版状态存储** | `~/.codex\` 下 5 组 SQLite：`state_5 / logs_2 / memories_1 / queue_1 / goals_1`（含 -shm/-wal）+ `sqlite\` 目录 | 同 | [本机已验证] | 本机 `ls` |
| 旧版会话（社区惯例） | `~/.codex\sessions\YYYY\MM\DD\rollout-*.jsonl`、`history.jsonl`、`log\`（log_dir 可配） | 同 | [社区报告]（官方 developers.openai.com 页面对本环境 403） | [issue #14389](https://github.com/openai/codex/issues/14389)、[会话迁移讨论 #46461](https://github.com/openai/codex/discussions/46461) |
| 扩展 | `~/.codex\plugins\ skills\ vendor_imports\ .sandbox*  .tmp\` | 同 | [本机已验证] | 本机 `ls` |
| 项目工作目录 | 用户自选（`--cd`/工作目录） | 同 | [官方文档] | codex 文档 |

⚠️ **重要异常**：本机**完全不存在** `sessions/`、`log/`、`history.jsonl` → 新版可能已把会话/状态迁入 SQLite。**TraceNest 探测器必须做"JSONL + SQLite"双分支**（[待验证]：SQLite 表结构逆向）。

## 2. Claude Code（Anthropic）——信息最全，官方文档整页可用

来源：[官方 .claude 目录文档](https://code.claude.com/docs/en/claude-directory)（本次研究全文抓取）。

| 目录类型 | Windows | macOS/Linux | 状态 |
|---|---|---|---|
| 主目录 | `%USERPROFILE%\.claude`（`CLAUDE_CONFIG_DIR` 可整体重定向，含 `~/.claude.json`） | `~/.claude` | [官方文档] |
| **会话转录** | `~/.claude\projects\<project>\<session>.jsonl`（明文 JSONL） | 同 | [官方文档] |
| **编码规则（官方明确）** | 工作目录路径中**除字母数字外的每个字符替换为 `-`**：`/home/u/w/my-repo` → `-home-u-w-my-repo`；Windows `D:\...` → `D--...`（Windows 实例为推断 [待验证]） | 同 | [官方文档]+[待验证] |
| 同树其他内容 | `<session>/subagents\|tool-results\|mcp-tasks`、`file-history/<session>/`（检查点快照，近 100 个）、`plans/`、`debug/`、`paste-cache/`、`image-cache/`、`uploads/`、`tasks/`、`session-env/`、`memory/`（项目级自动记忆）、`backups/`（保留 5 份）、`usage-data/`、`plugins/`、`sessions/`、`history.jsonl`、`.credentials.json` | 同 | [官方文档] |
| 旧版遗留（不再写入） | `statsig/ todos/ logs/` | 同 | [官方文档] |
| 清扫策略 | `cleanupPeriodDays` 默认 **30 天**自动清理 | 同 | [官方文档] |
| 临时区 | `%TEMP%\claude\<project>\<session>\scratchpad\`（`CLAUDE_CODE_TMPDIR` 可改） | `/tmp/claude/...` | [官方文档] |
| 本机状态 | `~/.claude` 仅有 `settings.json`；`.claude.json`、`projects/` 均不存在 | — | [本机已验证] |

**对 TraceNest 的意义**：Claude Code 是**唯一有官方编码规则的会话↔项目映射**——F5（会话映射）从这里开始最稳。隐私红线：`.jsonl` 是真实聊天记录（含系统提示/工具定义），默认只读元数据。

## 3. Cursor（Anysphere）

官方文档仅证实 `~/.cursor\mcp.json`（全局）与项目级 `.cursor\mcp.json`（[官方 MCP 文档](https://cursor.com/docs/context/mcp)）；其余靠社区：

| 目录类型 | Windows | 状态 | 来源 |
|---|---|---|---|
| 用户级 | `%USERPROFILE%\.cursor\`（mcp.json、chats、agent-transcripts——版本相关） | [社区报告]（本机未装） | 论坛/社区 |
| 应用数据 | `%APPDATA%\Cursor\`：Chromium 式 `Cache`、`Code Cache`、`GPUCache`、`Service Worker`、`User\globalStorage`、`User\workspaceStorage` | [社区报告] | VSCode-fork 惯例+论坛 |
| 安装/更新 | `%LOCALAPPDATA%\Programs\cursor`；`%LOCALAPPDATA%\cursor-updater` | [社区报告]【待验证】 | 惯例 |
| **最大缓存点** | `User\globalStorage\state.vscdb`（SQLite，`cursorDiskKV` 表）：论坛实证**单日增长 1GB**、有用户总占用 **125GB**；清理必须避开 state.vscdb（设置与历史同库）；`CachedData` 陈旧目录无自动清理 | [社区报告] | [论坛实证帖](https://forum.cursor.com/t/151747)、[50–100GB 案例](https://forum.cursor.com/t/agent-chat-db-state-vscdb-grows-without-bound-50-bubble-rows-per-visible-message-96-gb-and-climbing/170508/9) |

→ TraceNest"大文件哨兵"的**第一优先级目标**。

## 4. Gemini CLI（Google）

来源：[官方 settings 文档](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/settings.md)、[checkpointing 文档](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/checkpointing.md)（官方仓库 docs 抓取）。

| 目录类型 | 路径 | 状态 |
|---|---|---|
| 设置 | `~/.gemini\settings.json`（全局）；项目 `.gemini/settings.json`（workspace 覆盖用户） | [官方文档] |
| 检查点（现行） | `~/.gemini\history\<project_hash>\`（shadow git 仓库）+ `~/.gemini\tmp\<project_hash>\checkpoints\`（会话 JSON）。旧文档的 `/tmp/gemini` 已被取代 | [官方文档] |
| 凭据 | `~/.gemini\oauth_creds.json` | [官方文档]+[社区报告] |
| 缺口 | `<project_hash>` 哈希算法**官方未说明** → 阻碍项目↔检查点反向映射 | [待验证] |
| 本机 | 未安装 | [本机已验证] |

## 5. OpenCode

来源：[官方 config 文档](https://opencode.ai/docs/config/)、[官方 CLI 文档](https://opencode.ai/docs/cli/)。

| 目录类型 | 路径 | 状态 |
|---|---|---|
| 配置 | `~/.config/opencode\opencode.json`（+ tui.json）；项目 `opencode.json`/`.opencode/`；环境变量 `OPENCODE_CONFIG[_DIR]` | [官方文档] |
| 托管配置 | Windows `%ProgramData%\opencode`；macOS `/Library/Application Support/opencode/`；Linux `/etc/opencode/` | [官方文档] |
| 会话存储 | 现行 `~/.local/share/opencode\opencode.db`（**SQLite**）；旧版 `storage/` 下 session/message/part 三层 JSON | [社区报告]（[存储分析](https://vibe-replay.com/blog/opencode-local-storage)） |
| 凭据 | `~/.local/share/opencode\auth.json` | [官方文档] |
| Windows 数据目录映射 | **[待验证]**（XDG 在 Windows 的具体落点需实测） | [待验证] |
| 本机 | 未安装 | [本机已验证] |

## 6. ZCode CLI（本机工具）

无公开官方文档；本机内置指南（zcode-configuration-guide）记载了配置体系，本机 `ls` 证实结构：

| 目录类型 | 路径与内容 | 状态 |
|---|---|---|
| 主目录 | `%USERPROFILE%\.zcode\`：`cli/ plugin-workspace/ skills/ tmp/ v2/ workspace/` | [本机已验证] |
| cli/ 内部 | `agents, artifacts, config.json, db, exec, image-cache, log, memories, node-fallback, pdf-cache, plugins, rollout` | [本机已验证] |
| v2/ 内部 | `bot-config.v3.json, bot-state.v3.json, credentials.json, setting.json` | [本机已验证] |
| 配置体系 | `~/.zcode/cli/config.json`（MCP/hooks/plugins）、`~/.zcode/AGENTS.md`、`skills\|commands/`（fallback `~/.agents/`）、工作区 `<repo>/.zcode/config.json` + `<repo>/AGENTS.md` | [官方文档·内置指南] |
| 大文件候选 | `db/`、`rollout/`、`image-cache/`、`pdf-cache/`、`workspace/` | [本机观察] |
| 格式文档 | rollout/db 内部格式、v2 用途无文档 | **[待验证]** |

## 7. WorkBuddy（= 腾讯 CodeBuddy Code）

| 项目 | 结论 | 状态 |
|---|---|---|
| 身份 | **腾讯 CodeBuddy Code 的品牌**；workbuddy.ai 有官方文档站与 CLI 发布说明（JS 渲染，正文无法抓取） | [社区报告]+官方站存在性确认 |
| 主目录 | `~/.codebuddy` + `CODEBUDDY_HOME` 环境变量（Windows 默认 `%USERPROFILE%` 下） | [社区报告]（两个独立第三方工具文档交叉） |
| 本机实况 | 本机实际存在的是 `~/.workbuddy\`（非 `.codebuddy`）：`app/ binaries/ blobs/ cache/ artifact-index/ audit-log/ changes-index/ changes-detail/ connectors/ cli-env/ clipboard-images/ device-id*` + UUID 子目录 + SQLite 迁移标记 | [本机已验证] |
| 内部布局 | 会话/缓存/日志的子目录语义 | **[待验证]**（官方文档抓取失败） |

> ⚠️ 疑点：`.workbuddy` 与第三方文档所称 `.codebuddy` 并存——可能为不同版本/品牌的两种布局，规则库需**双路径同时探测**。[待验证]

## 8. DSH（= DeepSeek Harness）

| 项目 | 结论 | 状态 |
|---|---|---|
| 身份 | **DeepSeek Harness**：官方仓库 [deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness)（Cordis 插件架构、TS/Node、`npx @deepseek-ai/dsh`、Web UI 127.0.0.1:3080） | [官方文档]（仓库存在） |
| 磁盘路径 | README **未记载任何路径**；社区（可信度低）称 `DSH_HOME` 默认 `~/.dsh`、会话绑定工作区 | [待验证] |
| 本机实况 | `~/.dsh\`：`sessions/ attachments/ profiles/ storages/ dsh-runtimes/ llm-deepseek/ .credentials.yaml .anonymous-user-id` | [本机已验证] |
| 下一步 | 读其 Cordis storage 域源码确认路径语义 | [待验证] |

## 9. 豆包（字节跳动，桌面版）

| 目录类型 | 路径 | 状态 |
|---|---|---|
| 应用数据 | `%APPDATA%\Doubao\public_config.json` | [本机已验证] |
| 用户数据 | `%LOCALAPPDATA%\Doubao\Application\` + `%LOCALAPPDATA%\Doubao\User Data\`（Chromium profile） | [本机已验证] |
| 社区线索 | 互相矛盾：一说"电脑版实为 PWA（数据在浏览器配置档）"，另有缓存清理实操文 | [社区报告]·可靠性低 |
| 大小特征 | 按 Electron/Chromium 惯例 `User Data` 下缓存随使用增长（未读内容） | 推断 |
| macOS | `~/Library/Application Support/Doubao` | [待验证] |

## 10. 共享缓存（AI 工具生态常见大文件来源，均官方文档级）

| 缓存 | Windows | macOS/Linux | 本机 | 来源 |
|---|---|---|---|---|
| HuggingFace | `%USERPROFILE%\.cache\huggingface\hub\models--*`（可达数百 GB） | `~/.cache/huggingface` | ❌ | [HF 官方文档](https://huggingface.co/docs/huggingface_hub/guides/manage-cache) |
| Ollama | `%USERPROFILE%\.ollama\models` | `~/.ollama/models` | ❌ | [Ollama FAQ](https://github.com/ollama/ollama/blob/main/docs/faq.md) |
| pip | `%LOCALAPPDATA%\pip\Cache` | `~/.cache/pip` | ✅ | [pip 文档](https://pip.pypa.io/en/stable/cli/pip_cache/) |
| npm | `%LOCALAPPDATA%\npm-cache` | `~/.npm` | ✅ | [npm folders](https://docs.npmjs.com/cli/v11/configuring-npm/folders) |
| pnpm | `%LOCALAPPDATA%\pnpm\store`（版本相关） | `~/.local/share/pnpm/store` | ❌ | [pnpm 文档](https://pnpm.io/cli/store) |
| cargo | `%USERPROFILE%\.cargo\registry` | `~/.cargo/registry` | ❌ | cargo 文档 |
| uv | `%LOCALAPPDATA%\uv\cache` | `~/.cache/uv` | ❌ | [uv 文档](https://docs.astral.sh/uv/concepts/cache) |
| Go | `%USERPROFILE%\go\pkg\mod` | `~/go/pkg/mod` | ❌ | [Go 文档](https://pkg.go.dev/cmd/go) |
| conda | `%USERPROFILE%\miniconda3\envs + pkgs\` | 同构 | ❌ | conda 文档 |

---

## 11. 对规则库 v1 的直接结论

1. **可直接入库（高置信）**：Codex（双分支：SQLite 新版 + JSONL 旧版）、Claude Code（官方整页文档）、Gemini CLI、OpenCode、豆包（本机+惯例）、共享缓存全家桶。
2. **入库但标低置信（[待验证·本机观察]）**：ZCode（rollout/db 格式无文档）、WorkBuddy（双路径疑点）、DSH（官方仓库无路径说明）。
3. **入库但本机不适用**：Cursor——规则必须内置：它是其他用户机器上最大的 AI 磁盘痛点（state.vscdb 1GB/天实证）。
4. **信号文件层（跨平台通用）**：`CLAUDE.md`、`AGENTS.md`、`.cursorrules`、`GEMINI.md`、`.cursorignore`、`.opencode/`、`.claude/`（项目级）。
5. **四家会话形态需各自适配器**：Claude（JSONL+编码路径）/ Codex（按日 rollout→新版 SQLite）/ OpenCode（SQLite）/ Gemini（project_hash 哈希目录，算法未公开）。
6. **探测策略**：目录探测需支持"存在性 + 内容格式"双分支，并按机器上实际版本校准（Claude Code 官方 `claude purge --dry-run` 类命令可作自校验参照）。
7. **信息缺口即社区机会**：ZCode/WorkBuddy/DSH 无完整公开文档 → 正是"社区适配器规则包"（F2）的立项依据。
