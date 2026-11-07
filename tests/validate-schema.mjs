#!/usr/bin/env node
/**
 * Schema 真实验证：用 Node 内置 SQLite（node:sqlite）执行
 * tests/schema.sql（v1 基线）+ tests/schema_v5.sql（M3-A）+ tests/schema_v6.sql
 * （M3-B1）+ v2~v6 补列，验证 DDL 语法/约束/索引/FTS5/外键。
 * Rust 侧迁移与差分逻辑由 cargo test 覆盖。
 * 用法：node tests/validate-schema.mjs   （退出码 0 = 通过）
 */
import { readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ddl = [
  readFileSync(join(HERE, "schema.sql"), "utf8"),
  readFileSync(join(HERE, "schema_v5.sql"), "utf8"),
  readFileSync(join(HERE, "schema_v6.sql"), "utf8"),
  readFileSync(join(HERE, "schema_v7.sql"), "utf8"),
  readFileSync(join(HERE, "schema_v8.sql"), "utf8"),
].join("\n");

const db = new DatabaseSync(":memory:");
try {
  db.exec(ddl);
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
    ALTER TABLE scans ADD COLUMN scan_scope TEXT;
    ALTER TABLE scans ADD COLUMN coverage_gaps TEXT;
  `);

  const tables = db
    .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    .all()
    .map((r) => r.name);
  const expected = [
    "meta", "volumes", "scans", "paths", "paths_fts", "tools",
    "rule_packs", "rules", "attributions", "agg_tool_size", "events", "watch_roots",
    // v5（M3-A）
    "file_entities", "file_events", "source_evidence", "source_candidates", "source_confirmations",
    // v6（M3-B1）
    "scan_diff_runs",
    // v7（M3-C）
    "correlation_candidates",
    // v8（M3-D）
    "browser_download_sources", "browser_downloads",
  ];
  const missing = expected.filter((t) => !tables.includes(t));
  if (missing.length) {
    console.error("❌ 缺少表:", missing.join(", "));
    process.exit(1);
  }

  // 冒烟：v1 基础读写
  db.exec("INSERT INTO meta(key,value) VALUES('schema_version','8')");
  db.exec("INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen,volume_guid) VALUES('C:\\','系统',199,62,'NTFS',0,0,'\\\\?\\Volume{11111111-2222-3333-4444-555555555555}')");
  db.exec("INSERT INTO scans(started_at,status,roots,app_version,scan_scope,coverage_gaps) VALUES(0,'done','[\"C:\\\\\"]','0.1.0','{\"roots\":[\"C:\\\\\"],\"exclude_dir_names\":[],\"follow_symlinks\":false}','{\"paths\":[],\"truncated\":false}')");
  const scanId = db.prepare("SELECT id FROM scans").get().id;
  db.exec(`INSERT INTO paths(volume_id,name,depth,is_dir,size,scan_id) VALUES(1,'Users',0,1,0,${scanId})`);
  db.exec("INSERT INTO paths_fts(rowid,name) VALUES(1,'Users')");
  const hit = db.prepare("SELECT name FROM paths_fts WHERE paths_fts MATCH 'user*'").get();

  // 外键：孤立 path 必须被拒
  let fkRejected = false;
  try {
    db.exec("INSERT INTO paths(volume_id,name,depth,is_dir,scan_id) VALUES(1,'x',0,0,999)");
  } catch { fkRejected = true; }
  if (!fkRejected) { console.error("❌ 外键约束未生效：孤立 path 插入成功"); process.exit(1); }

  // v5 冒烟 + v6 台账/事件
  db.exec("INSERT INTO file_entities(volume_id,identity_kind,file_id,check_size,check_mtime,current_path_id,first_seen_scan_id,last_seen_scan_id,first_seen_at,last_seen_at) VALUES(1,'file_id',4242,100,0,1,1,1,0,0)");
  const entId = db.prepare("SELECT id FROM file_entities").get().id;
  db.exec(`INSERT INTO source_evidence(kind,origin,collected_at,payload) VALUES('user_statement','user',${Date.now()},'{}')`);
  const evId = db.prepare("SELECT id FROM source_evidence").get().id;
  db.exec(`INSERT INTO source_confirmations(entity_id,action,behavior,created_at) VALUES(${entId},'confirm','ai_created',0)`);
  db.exec(`INSERT INTO file_events(entity_id,ts,kind,evidence_id) VALUES(${entId},0,'source_confirmed',${evId})`);
  // v6：台账（UNIQUE）+ 差分事件（CHECK/挂点/唯一索引/diff_run_id FK）
  db.exec(`INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,created_at) VALUES(1,${scanId},${scanId},'completed',0,0)`);
  let dupRunRejected = false;
  try {
    db.exec(`INSERT INTO scan_diff_runs(volume_id,from_scan_id,to_scan_id,status,events_count,created_at) VALUES(1,${scanId},${scanId},'completed',0,0)`);
  } catch { dupRunRejected = true; }
  if (!dupRunRejected) { console.error("❌ 台账 UNIQUE(volume,to_scan) 未生效"); process.exit(1); }
  db.exec(`INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,diff_run_id,scan_id) VALUES(1,1,'users','USERS',0,'file_discovered',1,${scanId})`);
  let checkRejected = false;
  try { db.exec("INSERT INTO file_events(ts,kind) VALUES(0,'file_discovered')"); } catch { checkRejected = true; }
  if (!checkRejected) { console.error("❌ v6 CHECK（挂点至少其一）未生效"); process.exit(1); }
  let dupEventRejected = false;
  try {
    db.exec(`INSERT INTO file_events(path_id,volume_id,rel_path,rel_key,ts,kind,diff_run_id,scan_id) VALUES(1,1,'users','USERS',1,'file_discovered',1,${scanId})`);
  } catch { dupEventRejected = true; }
  if (!dupEventRejected) { console.error("❌ 事件唯一索引 (kind,path_id,scan_id) 未生效"); process.exit(1); }

  console.log("✅ Schema 验证通过（v1 基线 + v5 + v6 + v7 + v8 + 补列）");
  console.log("   表数量:", tables.length, "| 期望表全部存在（含 v5 五表 + v6 台账 + v7 关联候选 + v8 下载记录）");
  console.log("   FTS5:", hit && hit.name === "Users" ? "搜索命中 ✓" : "搜索异常 ✗");
  console.log("   外键/CHECK/唯一索引:", "孤立记录、双空挂点、重复台账、重复事件均被拒绝 ✓");
} catch (e) {
  console.error("❌ Schema 验证失败:", e.message);
  process.exit(1);
} finally {
  db.close();
}
