#!/usr/bin/env node
/**
 * 生成 FMap 应用图标（Windows ICO，32x32 32bpp BMP 帧）。
 * M0 临时方案：程序化生成一个蓝紫渐变 + "F" 缺省图形；
 * 正式 Logo 后续替换（重新运行本脚本即可）。
 * 用法：node scripts/make-icon.mjs
 */
import { writeFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "apps", "desktop", "src-tauri", "icons");
const SIZE = 32;

/** 取 [x,y] 处的 BGRA 像素：蓝→紫对角渐变，中间画一个白色"F"笔画。 */
function pixel(x, y) {
  const t = (x + y) / (2 * SIZE);
  // 深蓝 #0b57d0 → 紫 #8a7bf0
  const r = Math.round(11 + (138 - 11) * t);
  const g = Math.round(87 + (123 - 87) * t);
  const b = Math.round(208 + (240 - 208) * t);
  // "F" 的一竖两横（粗 3px）
  const left = x >= 8 && x <= 10 && y >= 7 && y <= 24;
  const topBar = y >= 7 && y <= 9 && x >= 8 && x <= 21;
  const midBar = y >= 14 && y <= 16 && x >= 8 && x <= 18;
  if (left || topBar || midBar) return [255, 255, 255, 255];
  return [b, g, r, 255];
}

// BITMAPINFOHEADER：高度 = 图像高*2（XOR+AND 双掩码）
const header = Buffer.alloc(40);
header.writeUInt32LE(40, 0);
header.writeInt32LE(SIZE, 4);
header.writeInt32LE(SIZE * 2, 8);
header.writeUInt16LE(1, 12);
header.writeUInt16LE(32, 14);

const xor = Buffer.alloc(SIZE * SIZE * 4);
for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    const [b, g, r, a] = pixel(x, SIZE - 1 - y); // BMP 行序自下而上
    const off = (y * SIZE + x) * 4;
    xor[off] = b; xor[off + 1] = g; xor[off + 2] = r; xor[off + 3] = a;
  }
}
const andMask = Buffer.alloc(SIZE * 4 * SIZE); // 全 0（alpha 通道已表达）

const image = Buffer.concat([header, xor, andMask]);
const dir = Buffer.alloc(6);
dir.writeUInt16LE(0, 0);
dir.writeUInt16LE(1, 2);
dir.writeUInt16LE(1, 4);
const entry = Buffer.alloc(16);
entry[0] = SIZE; entry[1] = SIZE; entry[2] = 0; entry[3] = 0;
entry.writeUInt16LE(1, 4);
entry.writeUInt16LE(32, 6);
entry.writeUInt32LE(image.length, 8);
entry.writeUInt32LE(22, 12); // 6 + 16

const ico = Buffer.concat([dir, entry, image]);
mkdirSync(OUT, { recursive: true });
writeFileSync(join(OUT, "icon.ico"), ico);
console.log(`✅ 生成 ${join(OUT, "icon.ico")}（${ico.length} 字节）`);
