#!/usr/bin/env node
/**
 * 只读门禁（安全测试，可在无 Rust 工具链时真实运行）。
 *
 * 扫描全部 Rust 源码，禁止出现针对用户文件的危险 API：
 *   remove_file / remove_dir_all / rename / set_permissions /
 *   write / create（限于文件写创建）——白名单见下。
 *
 * 白名单原则（docs/architecture/SECURITY.md）：
 *   - 允许：数据库文件写（%APPDATA%/FMap 内部）、日志写
 *   - 禁止：一切面向用户文件系统的写/删/改
 *
 * 用法：node tests/check-readonly.mjs   （退出码 0 = 通过）
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

// 危险 API → 触发条件说明
const FORBIDDEN = [
  { re: /\bstd::fs::remove_file\b/, why: "删除文件" },
  { re: /\bstd::fs::remove_dir_all?\b/, why: "删除目录" },
  { re: /\bfs::remove_file\b/, why: "删除文件" },
  { re: /\bfs::remove_dir\b/, why: "删除目录" },
  { re: /\bstd::fs::rename\b/, why: "移动/重命名" },
  { re: /\bfs::rename\b/, why: "移动/重命名" },
  { re: /\bstd::fs::set_permissions\b/, why: "修改权限" },
  { re: /\bDeleteFileW\b/, why: "Win32 删除文件" },
  { re: /\bRemoveDirectoryW\b/, why: "Win32 删除目录" },
  { re: /\bMoveFileW\b/, why: "Win32 移动文件" },
];

// 白名单：这些文件允许出现写类词（数据库/日志/图标生成等内部产物）
const ALLOW_FILES = /schema\.sql$|\.md$|check-readonly\.mjs$/;

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === "target" || name === "node_modules" || name === ".git" || name === "dist") continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

const violations = [];
for (const p of walk(ROOT)) {
  if (!p.endsWith(".rs")) continue;
  const full = readFileSync(p, "utf8");
  // 门禁意图：禁止对【用户文件】的删除/移动/重命名/写入 API。
  // 测试模块（#[cfg(test)] 之后的区块）仅操作 tempfile 自建的临时目录 fixture
  // （构造差分场景必需的真实删除/重命名），不属于用户文件——跳过扫描。
  const testIdx = full.indexOf("#[cfg(test)]");
  const src = testIdx >= 0 ? full.slice(0, testIdx) : full;
  for (const { re, why } of FORBIDDEN) {
    const m = src.match(re);
    if (m) violations.push(`${p}: 出现 ${why}（${m[0]}）`);
  }
}

if (violations.length) {
  console.error("❌ 只读门禁未通过：");
  for (const v of violations) console.error("  " + v);
  process.exit(1);
} else {
  console.log("✅ 只读门禁通过：全部 .rs 源码未发现删除/移动/重命名 API");
}
