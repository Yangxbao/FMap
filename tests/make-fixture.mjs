#!/usr/bin/env node
/**
 * 合成文件树生成器（M1 基准/验收用）。
 * 生成确定性的目录+文件树到 tests/fixture-tree/，供：
 *  - cargo test（工具链就绪后）与扫描基准
 *  - 人工核对扫描结果（文件数/目录数可精确对账）
 * 用法：node tests/make-fixture.mjs [目录数=200] [每目录文件数=5]
 * 只在 tests/fixture-tree 内创建文件；运行前先清空该目录（仅限本目录）。
 */
import { rmSync, mkdirSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "fixture-tree");
const [dirCount = 200, filesPerDir = 5] = process.argv.slice(2).map(Number);

// 清理旧 fixture（仅限 fixture-tree 目录内部，硬编码防误删）
rmSync(OUT, { recursive: true, force: true });
mkdirSync(OUT, { recursive: true });

let files = 0, dirs = 0;
for (let d = 0; d < dirCount; d++) {
  const sub = join(OUT, `dir-${String(d).padStart(4, "0")}`, `nested-${d % 3}`);
  mkdirSync(sub, { recursive: true });
  dirs += 2; // dir-XXXX + nested-X（可能重复创建，下面按实际统计修正）
  for (let f = 0; f < filesPerDir; f++) {
    writeFileSync(join(sub, `file-${f}.txt`), Buffer.alloc(64 + f, 65)); // 'A' * n
    files++;
  }
}
// 修正目录计数：重新遍历统计真实目录数
import { readdirSync, statSync } from "node:fs";
function countDirs(p) {
  let n = 0;
  for (const e of readdirSync(p)) {
    const s = statSync(join(p, e));
    if (s.isDirectory()) { n += 1 + countDirs(join(p, e)); }
  }
  return n;
}
dirs = countDirs(OUT);
console.log(`✅ 合成文件树已生成: ${OUT}`);
console.log(`   目录 ${dirs} 个 · 文件 ${files} 个（确定性结构，可精确对账）`);
