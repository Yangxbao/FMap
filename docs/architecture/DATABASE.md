# FMap 数据库设计（DATABASE）

> 版本：v0.9（夜间研究稿） · 日期：2026-11-03
> 引擎：SQLite（rusqlite + bundled），WAL 模式，`foreign_keys=ON`
> 位置：`%APPDATA%/FMap/tracenest.db`（设置中可更改位置）

---

## 1. 设计原则

1. **只存元数据，绝不存文件内容**（隐私红线）；文件名/路径本身就是敏感信息，因此 DB 视为敏感数据（见 SECURITY.md）。
2. 以 **扫描代（scan generation）** 为单位组织数据：每次全量/增量扫描一个代号，便于快照差分（V0.2）与回滚。
3. 大表必须可分页、可聚合：查询走索引，treemap 用预聚合表。
4. 规模预算：目标支撑 **100 万文件级** 单盘 / 数百万行总量。按行均 ~200B（含索引）估算，DB 体积约 0.5–2 GB 级别；提供"压缩/清理旧扫描代"入口。

---

## 2. 核心表结构（V0.1 DDL 草案）

```sql
-- 卷与扫描
CREATE TABLE volumes (
  id INTEGER PRIMARY KEY,
  root TEXT NOT NULL UNIQUE,          -- 'C:\' / 'D:\'
  label TEXT, total_bytes INTEGER, free_bytes INTEGER,
  first_seen INTEGER NOT NULL         -- unix ms
);

CREATE TABLE scans (
  id INTEGER PRIMARY KEY,             -- 扫描代
  started_at INTEGER NOT NULL, finished_at INTEGER,
  status TEXT NOT NULL DEFAULT 'running',  -- running|done|cancelled|failed
  roots TEXT NOT NULL,                -- JSON 数组
  tool_files_scanned INTEGER, tool_version TEXT
);

CREATE TABLE paths (
  id INTEGER PRIMARY KEY,             -- 稳定节点 id（同卷内唯一）
  volume_id INTEGER NOT NULL REFERENCES volumes(id),
  parent_id INTEGER REFERENCES paths(id),
  name TEXT NOT NULL,                 -- 不含分隔符
  depth INTEGER NOT NULL,
  is_dir INTEGER NOT NULL,
  size INTEGER NOT NULL DEFAULT 0,    -- 目录 = 子树聚合（重算时机见 §3）
  mtime INTEGER, ctime INTEGER,
  attrs INTEGER,                      -- 原始属性位（含 reparse/hardlink 标记）
  scan_id INTEGER NOT NULL REFERENCES scans(id),
  deleted_at INTEGER                  -- 增量扫描发现消失时填充（软删除）
);
CREATE INDEX idx_paths_parent ON paths(parent_id);
CREATE INDEX idx_paths_volume_name ON paths(volume_id, name);
CREATE INDEX idx_paths_size ON paths(volume_id, is_dir, size DESC);
CREATE INDEX idx_paths_mtime ON paths(volume_id, mtime);

-- 文件名搜索
CREATE VIRTUAL TABLE paths_fts USING fts5(name, content='');
-- 同步触发器/维护逻辑在 Rust 层批量维护

-- AI 工具与规则
CREATE TABLE tools (
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,           -- 'claude-code' / 'codex' / ...
  name TEXT NOT NULL, vendor TEXT,
  color TEXT,                         -- UI 主色
  builtin INTEGER NOT NULL DEFAULT 1  -- 1=内置规则 0=社区规则包
);

CREATE TABLE rule_packs (
  id INTEGER PRIMARY KEY, pack_name TEXT NOT NULL,
  version TEXT NOT NULL, imported_at INTEGER NOT NULL,
  rule_count INTEGER, source TEXT     -- 'builtin' | 'local-import'
);

CREATE TABLE rules (
  id INTEGER PRIMARY KEY,
  pack_id INTEGER REFERENCES rule_packs(id),
  tool_id INTEGER NOT NULL REFERENCES tools(id),
  kind TEXT NOT NULL,                 -- 'dir' | 'signal'
  os TEXT NOT NULL,                   -- 'win'|'mac'|'linux'|'*'
  pattern TEXT NOT NULL,              -- dir: 路径模板；signal: 文件名
  location TEXT,                      -- signal: 'project-root'|'anywhere'
  confidence TEXT NOT NULL,           -- 'verified'|'likely'
  kind_tag TEXT,                      -- install|config|cache|session|log|download|project
  risk_level TEXT,                    -- safe|caution|critical (删除风险)
  doc_url TEXT, source TEXT,
  UNIQUE(pack_id, tool_id, kind, pattern, location)
);

-- 归属结论（materialized，可整体按 scan 重建）
CREATE TABLE attributions (
  path_id INTEGER NOT NULL REFERENCES paths(id),
  scan_id INTEGER NOT NULL,
  tool_id INTEGER REFERENCES tools(id),   -- NULL = unattributed
  confidence TEXT NOT NULL,           -- verified|likely|inferred|none
  score REAL NOT NULL DEFAULT 0,
  basis TEXT NOT NULL                 -- JSON: [{rule_id|signal|inherited-from:...}]
);
CREATE INDEX idx_attr_path ON attributions(path_id);
CREATE INDEX idx_attr_tool ON attributions(scan_id, tool_id, confidence);

-- 预聚合：目录子树内按工具的占用（treemap 着色/排行用，避免每次聚合百万行）
CREATE TABLE agg_tool_size (
  path_id INTEGER NOT NULL,           -- 目录节点
  scan_id INTEGER NOT NULL,
  tool_id INTEGER,                    -- NULL = 非 AI
  direct_bytes INTEGER NOT NULL DEFAULT 0,   -- 直接命中
  total_bytes INTEGER NOT NULL DEFAULT 0,    -- 含子树
  PRIMARY KEY (path_id, tool_id)
);

-- 实时事件（F11 展示层；仅 watcher 覆盖的根）
CREATE TABLE events (
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,                 -- create|modify|remove|rename_from|rename_to
  volume_id INTEGER REFERENCES volumes(id),
  path TEXT NOT NULL,                 -- 事件路径（可能未入库）
  size INTEGER,
  watch_root TEXT NOT NULL,
  scan_id INTEGER                     -- 关联到归属（若命中已知目录）
);
CREATE INDEX idx_events_ts ON events(ts DESC);

-- 监控与设置
CREATE TABLE watch_roots (
  id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE,
  enabled INTEGER NOT NULL DEFAULT 1, added_at INTEGER NOT NULL
);

CREATE TABLE meta (
  key TEXT PRIMARY KEY, value TEXT    -- schema_version, rule_pack_state, app_flags
);
```

> V0.2 追加：`projects`（逻辑项目）、`project_fingerprints`（内容哈希）、`snapshots`（快照差分基线）。DDL 届时随 PR 评审更新。

---

## 3. 维护与一致性策略

1. **写入路径**：扫描器批量写（事务 5k–10k 行 / prepared statements）；watcher 事件逐条低频写。WAL 允许写事务与前端只读查询并发。
2. **目录 size 聚合**：扫描时自底向上累计（闭包方式：叶子求和回填），写入 `paths.size`；`agg_tool_size` 在归属阶段一次性生成（按 scan 代）。
3. **FTS 同步**：扫描批次内同步写 `paths_fts`，不依赖触发器（大事务下触发器成本高）。
4. **删除策略**：文件级"消失"用 `deleted_at` 软删（支撑历史时间线）；旧扫描代数据由用户触发的"清理"硬删（`PRAGMA optimize` + `VACUUM` 定期）。
5. **迁移**：`meta.schema_version` + 启动时顺序迁移脚本（Rust 侧 `user_version` pragma 等价实现）。
6. **规模演练点（写入 V0.1 验收）**：用合成文件树（100 万文件）灌库后，`query_tree` / `query_files` / FTS 搜索 P95 ≤ 200ms。

---

## 4. 隐私相关约束（与 SECURITY.md 联动）

- DB 内路径 = 用户敏感数据：默认不落任何"文件内容"，不落网络地址，不落遥测。
- 提供一键导出（供用户自查 DB 有什么）与一键清空。
- DB 文件权限：创建时限制为当前用户（Windows 上 %APPDATA% 默认即用户私有，仍显式校验）。
- 未来若做 V0.2 内容哈希：哈希值单独表存放并可一键清除，且默认关闭。
