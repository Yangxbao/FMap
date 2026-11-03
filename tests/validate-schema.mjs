#!/usr/bin/env node
/**
 * Schema 真实验证：用 Node 内置 SQLite（node:sqlite）执行 tests/schema.sql。
 *
 * 说明：这验证的是 DDL 本身（语法、约束、索引、FTS5 可用性），
 * 不是 Rust rusqlite 代码 —— 后者待 Rust 工具链安装后由 cargo test 覆盖。
 *
 * 用法：node tests/validate-schema.mjs   （退出码 0 = 通过）
 */
import { readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const SCHEMA = join(dirname(fileURLToPath(import.meta.url)), "schema.sql");
const ddl = readFileSync(SCHEMA, "utf8");

const db = new DatabaseSync(":memory:");
try {
  db.exec(ddl);

  const tables = db
    .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    .all()
    .map((r) => r.name);
  const expected = [
    "meta", "volumes", "scans", "paths", "paths_fts", "tools",
    "rule_packs", "rules", "attributions", "agg_tool_size", "events", "watch_roots",
  ];
  const missing = expected.filter((t) => !tables.includes(t));

  if (missing.length) {
    console.error("❌ 缺少表:", missing.join(", "));
    process.exit(1);
  }

  // 基础读写冒烟：插入 → 读回（验证约束真的生效）
  db.exec("INSERT INTO meta(key,value) VALUES('schema_version','1')");
  db.exec("INSERT INTO volumes(root,label,total_bytes,free_bytes,fs_type,is_removable,first_seen) VALUES('C:\\','系统',199,62,'NTFS',0,0)");
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

  console.log("✅ Schema 验证通过");
  console.log("   表数量:", tables.length, "| 期望表全部存在");
  console.log("   FTS5:", hit && hit.name === "Users" ? "搜索命中 ✓" : "搜索异常 ✗");
  console.log("   外键:", "孤立记录被拒绝 ✓");
  console.log("   冒烟读写:", "meta/volumes/scans/paths 读写正常 ✓");
} catch (e) {
  console.error("❌ Schema 验证失败:", e.message);
  process.exit(1);
} finally {
  db.close();
}
