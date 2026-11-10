# Changelog

## v0.2.0 (2026-11-11)

### 新功能

- **AI 工具行为关联**（M3-C）：时间+路径双上下文门槛的关联候选系统——
  特征文件（±60 分钟）/数据目录（±24 小时）两类信号、完整证据链、
  确认/否认/忽略/撤回四向审查（重建永久保留已审查行）；绝不自动认定来源
- **浏览器下载记录接入**（M3-D）：Chrome/Edge 逐浏览器×逐档案授权（默认关闭）、
  History 只读副本解析（最小字段集，不触碰浏览历史/Cookie/密码）、
  下载记录×扫描路径候选匹配
- **USN Journal 能力探测**（M3-E）：单文件 USN 读取（FSCTL_READ_FILE_USN_DATA，
  无需管理员）+ 卷级能力探测（需管理员→红线内不启用，设计留档）；安全降级
- **磁盘地图 / 文件浏览 / 实时监控页**（M4）：卷级快照视图、面包屑导航、
  FTS+LIKE 双路搜索、归属详情抽屉、只读定位（explorer /select）
- **项目资产页**：以 AI 工具工作区为单元的资产聚合（子树统计+活动计数）
- **报告中心**：工具占用 / 卸载安全清单 / 空间变化（复制文本导出，永不写用户文件系统）
- **历史追踪页 v2**：分页 + 按卷过滤 + 路径子串搜索
- **差分超限处置**：上限可配置（meta）+ 提高上限自动重试 / 确认放弃；设置页上限卡
- **便携模式**：FMAP_DB_PATH 环境变量自定义数据库位置

### 性能

- 文件搜索：最新代过滤预计算 IN 列表（逐行相关子查询 15s→2.1s）、
  单字符查询直走 LIKE、绝对路径批量构建（N+1 消除）、FTS 匹配集内层截断
- Schema v9：file_events 时间线 / 关联候选 / 下载记录全局索引

### 修复

- 自定义路径扫描误触发全盘扫描（扫描范围语义与按钮禁用条件一致化）
- 行为关联信号链混入扫描根节点名导致候选恒为 0（坐标系与事件 rel 对齐）
- 中文文件名搜索失灵（FTS unicode61 对 CJK 前缀失配→LIKE 兜底）
- 磁盘地图下钻 hash 参数被导航覆盖
- tauri beforeBuildCommand 相对路径基准错误（该配置此前从未成功执行）

### 工程化

- NSIS 安装包：构建 + 真机静默安装/卸载/数据库连接全验证
- CI：npm --prefix 失效修复（working-directory + npm ci）、Node 20→24
  （node:sqlite）、clippy -D warnings 清零——CI 历史首次绿色
- UI 统一：PageHeader/StatStrip/EmptyWithAction 组件、全部 10 页真实实现
  （占位页清零）

### 数据库

- Schema v6→v9（correlation_candidates / browser_download_sources /
  browser_downloads / v9 全局索引；26 表；阶段事务非破坏迁移）

### 测试

- Rust 143 测试（core 38 · database 85 · platform 20）
- 前端 vitest 17 测试

---

## v0.1.0 (2026-11-07)

### 核心功能

- **只读多磁盘扫描引擎**（M1）：七态状态机、批量事务写入（110K 行/秒）、
  取消一致性、异常退出恢复（instance_id）、覆盖缺口收集
- **AI 工具识别与归属**（M2）：内置规则包（15 工具/29 规则）、四级证据归属引擎、
  Verified 目录继承、冲突检测
- **文件身份与来源确认**（M3-A）：卷 GUID + NTFS FileId 主身份（默认关闭）、
  来源确认/修正/撤销（append-only 三层分模型）、路径歧义隔离
- **基础文件事件追踪**（M3-B1）：扫描差分四类事件
  （DISCOVERED/MODIFIED/MISSING/REAPPEARED）、差分台账（原子提交+阻塞+恢复）、
  覆盖缺口抑制 MISSING（宁漏勿误）
- **实时目录监控**（M3-B2）：RDCW 逐目录授权（默认关闭）、原始通知层（零去重）、
  溢出自动对账、gap 生命周期

### 数据库

- Schema v1→v6（阶段事务+幂等补列+崩溃续跑）
- 23 表（meta/volumes/scans/paths/paths_fts/tools/rule_packs/rules/attributions/
  agg_tool_size/events/watch_roots/file_entities/file_events/source_evidence/
  source_candidates/source_confirmations/scan_diff_runs + FTS5 影子表）

### 测试

- Rust 93 测试（core 25 · database 56 · platform 12）
- 前端 vitest 11 测试
- 只读安全门禁 + Schema 验证

### 安全

- 只读门禁（禁止用户文件删除/修改/移动/重命名 API）
- 提权拒绝（管理员运行自动退出）
- 身份采集默认关闭
- 监控默认关闭
