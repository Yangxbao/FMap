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

/** 页面路由 id（侧栏 10 项；M3-C 新增"行为关联"）。 */
export const PAGE_IDS = [
  "overview",
  "diskmap",
  "files",
  "tools",
  "correlation",
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

/** 来源行为 → 中文标签（未知值原样返回，绝不编造）。 */
export function sourceBehaviorLabel(b: string): string {
  const map: Record<string, string> = {
    ai_created: "AI 创建",
    ai_modified: "AI 修改",
    ai_downloaded: "AI 下载",
    user_created: "用户创建",
    user_modified: "用户修改",
    unknown: "未知",
  };
  return map[b] ?? b;
}

/** 来源确认动作 → 中文标签。 */
export function sourceActionLabel(a: string): string {
  const map: Record<string, string> = {
    confirm: "确认",
    correct: "修正",
    revoke: "撤销",
  };
  return map[a] ?? a;
}

/** 文件事件类型 → 中文标签（M3-A 子集；M3-B 起的词原样返回）。 */
export function eventKindLabel(k: string): string {
  const map: Record<string, string> = {
    source_confirmed: "来源确认",
    source_corrected: "来源修正",
    file_id_reuse_suspected: "身份重用嫌疑（待确认）",
    file_discovered: "首次观察到",
    file_modified: "观察到变化",
    file_missing: "缺失（待确认）",
    file_reappeared: "重新出现",
  };
  return map[k] ?? k;
}

/** 差分台账状态 → 中文标签。 */
export function diffRunStatusLabel(st: string): string {
  const map: Record<string, string> = {
    completed: "已完成研判",
    skipped_scope_mismatch: "已跳过（扫描范围不一致）",
    skipped_over_limit: "已跳过（人工确认放弃超限对）",
    pending_over_limit: "已阻塞（事件量超限，待人工处理）",
  };
  return map[st] ?? st;
}

/** 差分事件 detail JSON 的 note 字段（语义备注）。 */
export function eventNote(detail: string | null): string {
  if (!detail) return "";
  try {
    const o = JSON.parse(detail) as { note?: string };
    return o.note ?? "";
  } catch {
    return "";
  }
}

// ---------- M3-C：工具行为关联 ----------

/** 候选审查状态 → 中文标签。 */
export function reviewStateLabel(s: string): string {
  const map: Record<string, string> = {
    pending: "待审查",
    confirmed: "已确认关联",
    denied: "已否认",
    ignored: "已忽略",
  };
  return map[s] ?? s;
}

/** 关联置信级别 → 中文标签（绝不出现"已验证"语义）。 */
export function corrConfidenceLabel(c: string): string {
  const map: Record<string, string> = {
    likely: "较强（特征文件信号）",
    possible: "较弱（数据目录信号）",
  };
  return map[c] ?? c;
}

/** 信号种类 → 中文标签。 */
export function corrSignalKindLabel(k: string): string {
  const map: Record<string, string> = {
    signal_path: "特征文件",
    data_dir: "工具数据目录",
  };
  return map[k] ?? k;
}

/** 时间差（毫秒）→ 人类可读（"20 分钟" / "3.5 小时"）；非正数返回占位。 */
export function fmtDelta(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms) || ms < 0) return "—";
  const MIN = 60 * 1000;
  if (ms < MIN) return `${ms / 1000} 秒`;
  if (ms < 60 * MIN) return `${(ms / MIN).toFixed(0)} 分钟`;
  if (ms < 24 * 60 * MIN) return `${(ms / (60 * MIN)).toFixed(1)} 小时`;
  return `${(ms / (24 * 60 * MIN)).toFixed(1)} 天`;
}
