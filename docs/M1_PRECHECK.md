# M1 预检报告（M1_PRECHECK）

> 日期：2026-11-03 · 依据：docs/M1_PROMPT_FROM_GPT.md（GPT 生成，用户授权执行）
> 步骤 1/2（检查现有项目与工具链）+ 步骤 3/4（架构分析与设计）+ 步骤 5（无需工具链的开发准备清单）

---

## 步骤 1：现有项目与工具链检查（真实结果）

| 项 | 结果 |
|---|---|
| Git 仓库 | ✅ main 分支 2 提交，工作区干净（`git status` 0 未提交） |
| 代码规模 | 1585 行 Rust/TS 源码（不含文档/原型） |
| 前端测试 | ✅ vitest 7/7（本轮复跑确认） |
| 仓库级测试 | ✅ schema 验证 + 只读门禁（本轮复跑确认） |
| Rust 工具链 | ❌ rustc/cargo/rustup 缺失（与 M0 时一致，未变化） |
| MSVC Build Tools | ❌ 缺失 |
| 本轮结论 | 可以开始"不依赖编译"的 M1 编码；编译验证挂起至人工安装 |

## 步骤 3：数据库结构与扫描架构分析

- `paths` 表已含扫描所需全部列（volume_id/parent_id/name/depth/is_dir/size/mtime/attrs/scan_id/deleted_at），4 个索引就位；`scans` 表承载七态状态与计数。
- 缺口 1：缺少**批量插入** DAO（M0 只有建表）。
- 缺口 2：缺少**扫描引擎**本体（遍历 + 状态机 + 取消 + 错误计数）。
- 缺口 3：`agg_tool_size` 依赖归属（M2），M1 不触碰。

## 步骤 4：扫描状态机 / IPC / 数据库写入策略（设计定稿）

**状态机**（与 core::ScanStatus 七态一一对应）：
```
Pending → Running → Done            （全部成功）
                  → Partial         （有跳过/无权目录，扫描正常结束）
                  → AccessDenied    （根目录本身无权读取）
                  → VolumeOffline   （根不存在/不可达）
                  → Failed          （意外错误）
任意时刻 → Cancelled（用户取消；DB 状态记 cancelled，与 Failed 区分）
```
不变量：**只有 Done 才允许 UI 显示"已完成"**；Partial 必须展示 denied_count；取消后已写入的批次保留并标记 cancelled（部分数据可用但明确标注），满足 GPT 验收问题 3"取消扫描后数据库一致"。

**IPC**（轮询式，M1 不引入事件通道以降低风险）：
- `scan_start(roots: string[])` → 返回 scan_id；后台线程执行
- `scan_status(scan_id)` → { status, files_scanned, dirs_scanned, denied_count, current_dir }
- `scan_cancel(scan_id)` → 设置 AtomicBool；walker 每目录检查

**数据库写入策略**：
- 每批次 ≤ 2000 条 `INSERT` 于单事务（prepared statement 复用）；批次间让出锁，保证 UI 查询不被长事务阻塞
- 目录 size 聚合：扫描期先存文件级 size=0；**聚合在扫描完成后单趟自底向上更新**（避免遍历中反复回写）
- parent_id：内存中维护 (相对路径 → id) 映射；根= volume 根节点

## 步骤 5：不依赖 Rust/MSVC 的开发准备清单（本报告即完成）

1. ✅ 扫描状态机与 IPC 契约定稿（上文）
2. ✅ 合成文件树生成器：`tests/make-fixture.mjs`（Node 可直接运行，供基准与人工核对）
3. ✅ 扫描引擎代码（步骤 6，见 crates/core/src/scanner.rs——**未编译标记**）
4. ✅ 数据库批量写入 DAO（**未编译标记**）
5. ✅ 前端扫描界面（步骤 7，真实调用 IPC；浏览器模式降级）
6. ⏸ 编译、cargo test、真机扫描——等人工安装（见 M1_HANDOFF §5 命令清单）

## 红线自检（步骤 13 预承诺）

本轮 M1 代码：不删除/移动/重命名任何文件（只读遍历）；不触碰 `~/.claude` 等会话内容；无网络调用；不自动安装任何软件。
