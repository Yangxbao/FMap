#!/usr/bin/env node
/**
 * Schema 真实验证：用 Node 内置 SQLite（node:sqlite）执行 tests/schema.sql
 * （v1 基线）+ tests/schema_v5.sql（M3-A 五表）+ v2~v5 补列。
 *
 * 说明：这验证的是 DDL 本身（语法、约束、索引、FTS5 可用性、外键），
 * 不是 Rust rusqlite 代码 —— 后者由 cargo test 覆盖（迁移/回填/确认等）。
 *
 * 用法：node tests/validate-schema.mjs   （退出码 0 = 通过）
 */
import { readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ddlV1 = readFileSync(join(HERE, "schema.sql"), "utf8");
const ddlV5 = readFileSync(join(HERE, "schema_v5.sql"), "utf8");

const db = new DatabaseSync(":memory:");
try {
  // v1 基线 + v5 增量表 + 与 Rust 迁移等价的补列（v2/v3/v4/v5）
  db.exec(ddlV1);
  db.exec(ddlV5);
  db.exec(`
    ALTER TABLE scans ADD COLUMN instance_id TEXT;
    ALTER TABLE scans ADD COLUMN end_note TEXT;
    ALTER TABLE rules ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
    ALTER TABLE rules ADD COLUMN weight REAL;
    ALTER TABLE rules ADD COLUMN evidence TEXT;
    ALTER TABLE tools ADD COLUMN kind TEXT;
    ALTER TABLE attributions ADD COLUMN source_behavior TEXT NOT NULL DEFAULT 'unknown';
    ALTER TABLE volumes ADD COLUMN volume_guid TEXT;
    ALTER TABLE attributions ADD COLUMN entity_id INTEGER REFERENCES file_entities(id);
  `);

  const tables = db
    .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    .all()
    .map((r) => r.name);
  const expected = [
    "meta", "volumes", "scans", "paths", "paths_fts", "tools",
    "rule_packs", "rules", "attributions", "agg_tool_size", "events", "watch_roots",
    // v5（M3-A 文件资产基础模型）
    "file_entities", "file_events", "source_evidence", "source_candidates", "source_confirmations",
  ];
  const missing = expected.filter((t) => !tables.includes(t));

  if (missing.length) {
    console.error("❌ 缺少表:", missing.join(", "));
    process.exit(1);
  }

  // 基础读写冒烟：插入 → 读回（验证约束真的生效）
  db.exec("INSERT INTO meta(key,value) VALUES('schema_version','5')");
  db.exec("INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen,volume_guid) VALUES('C:\\','系统',199,62,'NTFS',0,0,'\\\\?\\Volume{11111111-2222-3333-4444-555555555555}')");
  db.exec("INSERT INTO scans(started_at,status,roots,app_version) VALUES(0,'running','[\"C:\\\\\"]','0.1.0')");
  const scanId = db.prepare("SELECT id FROM scans").get().id;
  db.exec(`INSERT INTO paths(volume_id,name,depth,is_dir,size,scan_id) VALUES(1,'Users',0,1,0,${scanId})`);
  db.exec("INSERT INTO paths_fts(rowid,name) VALUES(1,'Users')");
  const hit = db.prepare("SELECT name FROM paths_fts WHERE paths_fts MATCH 'user*'").get();

  // 外键约束验证：无 scan 2 时插入应失败
  let fkRejected = false;
  try {
    db.exec("INSERT INTO paths(volume_id,name,depth,is_dir,scan_id) VALUES(1,'x',0,0,999)");
  } catch {
    fkRejected = true;
  }
  if (!fkRejected) {
    console.error("❌ 外键约束未生效：孤立 path 插入成功");
    process.exit(1);
  }

  // v5 冒烟：实体 → 事件 → 确认/证据；孤立事件必须被拒
  db.exec("INSERT INTO file_entities(volume_id,identity_kind,file_id,check_size,check_mtime,current_path_id,first_seen_scan_id,last_seen_scan_id,first_seen_at,last_seen_at) VALUES(1,'file_id',4242,100,0,1,1,1,0,0)");
  const entId = db.prepare("SELECT id FROM file_entities").get().id;
  db.exec(`INSERT INTO source_evidence(kind,origin,collected_at,payload) VALUES('user_statement','user',${Date.now()},'{}')`);
  const evId = db.prepare("SELECT id FROM source_evidence").get().id;
  db.exec(`INSERT INTO source_confirmations(entity_id,action,behavior,created_at) VALUES(${entId},'confirm','ai_created',0)`);
  db.exec(`INSERT INTO file_events(entity_id,ts,kind,evidence_id) VALUES(${entId},0,'source_confirmed',${evId})`);
  let orphanEventRejected = false;
  try {
    db.exec("INSERT INTO file_events(entity_id,ts,kind) VALUES(999,0,'x')");
  } catch {
    orphanEventRejected = true;
  }
  if (!orphanEventRejected) {
    console.error("❌ v5 外键未生效：孤立 file_events 插入成功");
    process.exit(1);
  }
  // 主身份部分唯一索引：同卷同 FileId 二次插入必须失败
  let dupRejected = false;
  try {
    db.exec("INSERT INTO file_entities(volume_id,identity_kind,file_id,first_seen_at,last_seen_at) VALUES(1,'file_id',4242,0,0)");
  } catch {
    dupRejected = true;
  }
  if (!dupRejected) {
    console.error("❌ v5 唯一索引未生效：同卷同 FileId 重复插入成功");
    process.exit(1);
  }
  // attributions.entity_id 关联列可用
  db.exec(`INSERT INTO tools(key,name,vendor,color,builtin) VALUES('codex','Codex','','#111',1)`);
  db.exec(`INSERT INTO attributions(path_id,scan_id,tool_id,confidence,score,basis,source_behavior,entity_id) VALUES(1,${scanId},1,'verified',1.0,'{}','unknown',${entId})`);

  console.log("✅ Schema 验证通过（v1 基线 + v5 增量 + 补列）");
  console.log("   表数量:", tables.length, "| 期望表全部存在（含 v5 五表）");
  console.log("   FTS5:", hit && hit.name === "Users" ? "搜索命中 ✓" : "搜索异常 ✗");
  console.log("   外键:", "孤立 paths/file_events 均被拒绝 ✓");
  console.log("   v5 唯一索引:", "同卷同 FileId 重复插入被拒绝 ✓");
  console.log("   v5 冒烟:", "实体/事件/确认/证据/归属关联 读写正常 ✓");
} catch (e) {
  console.error("❌ Schema 验证失败:", e.message);
  process.exit(1);
} finally {
  db.close();
}
