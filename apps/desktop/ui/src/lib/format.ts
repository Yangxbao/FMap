/** 展示层纯函数（无副作用，便于测试）。 */

/** 字节数 → 人类可读大小（与 Rust 侧 tracenest_core::format_bytes 语义一致）。 */
export function formatBytes(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes)) return "—";
  if (bytes < 0) return "—";
  const KB = 1024, MB = KB * 1024, GB = MB * 1024, TB = GB * 1024;
  if (bytes >= TB) return `${(bytes / TB).toFixed(1)} TB`;
  if (bytes >= GB) return `${(bytes / GB).toFixed(1)} GB`;
  if (bytes >= MB) return `${(bytes / MB).toFixed(1)} MB`;
  if (bytes >= KB) return `${(bytes / KB).toFixed(1)} KB`;
  return `${bytes} B`;
}

/** 页面路由 id（侧栏 9 项，与用户确认的 M0-2 清单一致）。 */
export const PAGE_IDS = [
  "overview",
  "diskmap",
  "files",
  "tools",
  "projects",
  "history",
  "monitor",
  "reports",
  "settings",
] as const;

export type PageId = (typeof PAGE_IDS)[number];

export function isPageId(v: string): v is PageId {
  return (PAGE_IDS as readonly string[]).includes(v);
}

/** 从 location.hash 解析当前页面（无效值回退 overview，绝不崩溃）。 */
export function pageFromHash(hash: string): PageId {
  const id = hash.replace(/^#\/?/, "");
  return isPageId(id) ? id : "overview";
}
