/**
 * IPC 封装层：前端访问 Rust 后端的唯一入口。
 *
 * 设计要点：
 * 1. 运行环境检测 —— 浏览器直开（无 Tauri 运行时）时抛出明确错误，
 *    UI 据此展示"后端未连接"状态，绝不伪造数据。
 * 2. 后端错误解析 —— Rust 侧 CommandError 序列化为 { kind, message }，
 *    这里还原为 IpcError，kind 用于 UI 分类提示。
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";

/** 是否运行在 Tauri 内（WebView2 注入了 IPC 桥）。 */
export const backendAvailable: boolean =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** 命令错误（与 Rust 侧 CommandError 对应）。 */
export class IpcError extends Error {
  readonly kind: string;
  constructor(kind: string, message: string) {
    super(message);
    this.kind = kind;
    this.name = "IpcError";
  }
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!backendAvailable) {
    throw new IpcError(
      "backend",
      "后端未连接：当前为浏览器直开模式，只能查看界面结构。请通过 Tauri 启动应用（npm run tauri dev）。"
    );
  }
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (raw: unknown) {
    if (isRecord(raw) && typeof raw.kind === "string" && typeof raw.message === "string") {
      throw new IpcError(raw.kind, raw.message);
    }
    if (raw instanceof Error) {
      throw new IpcError("backend", raw.message);
    }
    throw new IpcError("backend", String(raw));
  }
}

// ---------- 与 Rust 侧结构对应的数据类型 ----------

export interface PlatformCapabilities {
  can_list_volumes: boolean;
  can_scan_metadata: boolean;
  can_read_file_contents: boolean;
  can_watch: boolean;
}

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  elevated: boolean | null;
}

export interface SystemStatus {
  app: AppInfo;
  database_ready: boolean;
  database_error: string | null;
  database_path: string | null;
  capabilities: PlatformCapabilities;
}

export interface VolumeInfo {
  root: string;
  label: string | null;
  total_bytes: number | null;
  free_bytes: number | null;
  fs_type: string | null;
  is_removable: boolean;
}

export interface DbStats {
  schema_version: number;
  volumes: number;
  scans_total: number;
  scans_running: number;
  paths_total: number;
}

export interface ScanProgress {
  scan_id: number;
  /** pending|running|done|cancelled|access_denied|volume_offline|failed */
  status: string;
  files: number;
  dirs: number;
  denied: number;
  current_dir: string;
  phase: string;
  error: string | null;
}

// ---------- M3-A：文件身份采集与来源确认 ----------

export interface SourceOverview {
  collection_enabled: boolean;
  entities: number;
  with_file_id: number;
  weak_identity: number;
  confirmed_entities: number;
  candidates: number;
  events: number;
}

export interface IdentitySettings {
  enabled: boolean;
  overview: SourceOverview;
}

export interface RevokeStats {
  deleted_data: boolean;
  entities: number;
  events: number;
  evidence: number;
  candidates: number;
  confirmations: number;
}

export interface IdentityVolume {
  volume_id: number;
  root: string;
  fs_type: string | null;
  /** NTFS = FileId 主身份可用；其他卷走弱身份（仍可采集） */
  identity_supported: boolean;
  entity_count: number;
  volume_guid: string | null;
}

export interface BackfillProgress {
  /** idle|running|completed|cancelled|failed */
  status: string;
  volume_root: string | null;
  entries: number;
  entities_new: number;
  entities_updated: number;
  reuse_suspected: number;
  weak_identity: number;
  unmapped: number;
  denied: number;
  current_dir: string;
  error: string | null;
}

/** 来源行为（用户确认词表；unknown 为合法且永久保留的状态）。 */
export type SourceBehavior =
  | "ai_created"
  | "ai_modified"
  | "ai_downloaded"
  | "user_created"
  | "user_modified"
  | "unknown";

export interface EffectiveSource {
  behavior: SourceBehavior;
  action: string;
  created_at: number;
}

export interface ConfirmationRow {
  id: number;
  action: string;
  behavior: SourceBehavior;
  note: string | null;
  created_at: number;
}

export interface EventRow {
  id: number;
  ts: number;
  kind: string;
  detail: string | null;
  evidence_id: number | null;
}

export interface PathSourceDetail {
  path_id: number;
  entity: {
    id: number;
    identity_kind: string;
    /** 16 进制 FileId（file_id 身份时非空） */
    file_id: string | null;
    first_seen_at: number;
    last_seen_at: number;
  } | null;
  effective: EffectiveSource | null;
  confirmations: ConfirmationRow[];
  events: EventRow[];
}

/** 后端命令的唯一访问点（新增命令在此补充）。 */
export const api = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  getSystemStatus: () => invoke<SystemStatus>("get_system_status"),
  listVolumes: () => invoke<VolumeInfo[]>("list_volumes"),
  getDbStats: () => invoke<DbStats>("get_db_stats"),
  scanStart: (roots: string[]) => invoke<ScanProgress>("scan_start", { roots }),
  runAttribution: () => invoke<unknown>("run_attribution"),
  getToolsOverview: () => invoke<unknown>("get_tools_overview"),
  getToolDetail: (toolKey: string) => invoke<unknown>("get_tool_detail", { toolKey }),
  setRuleEnabled: (ruleId: number, enabled: boolean) =>
    invoke<boolean>("set_rule_enabled", { ruleId, enabled }),
  scanStatus: (scanId: number) => invoke<ScanProgress>("scan_status", { scanId }),
  scanCancel: (scanId: number) => invoke<boolean>("scan_cancel", { scanId }),
  // M3-A
  getIdentitySettings: () => invoke<IdentitySettings>("get_identity_settings"),
  setIdentityCollection: (enabled: boolean) =>
    invoke<boolean>("set_identity_collection", { enabled }),
  revokeIdentityCollection: (deleteData: boolean) =>
    invoke<RevokeStats>("revoke_identity_collection", { deleteData }),
  listIdentityVolumes: () => invoke<IdentityVolume[]>("list_identity_volumes"),
  identityBackfillStart: (root: string) =>
    invoke<BackfillProgress>("identity_backfill_start", { root }),
  identityBackfillStatus: () => invoke<BackfillProgress>("identity_backfill_status"),
  identityBackfillCancel: () => invoke<boolean>("identity_backfill_cancel"),
  getPathSourceDetail: (pathId: number) =>
    invoke<PathSourceDetail>("get_path_source_detail", { pathId }),
  confirmSource: (pathId: number, behavior: SourceBehavior, note: string | null) =>
    invoke<EffectiveSource>("confirm_source", { pathId, behavior, note }),
  revokeSource: (pathId: number, note: string | null) =>
    invoke<EffectiveSource>("revoke_source", { pathId, note }),
  // M3-B1 历史追踪
  getDiffRuns: (volumeId: number | null) =>
    invoke<DiffRunRow[]>("get_diff_runs", { volumeId }),
  getHistoryEvents: (limit: number) =>
    invoke<HistoryEventRow[]>("get_history_events", { limit }),
  runDiffNow: (volumeId: number) =>
    invoke<DiffSummary>("run_diff_now", { volumeId }),
  // M3-C 工具行为关联
  runCorrelation: () => invoke<CorrSummary>("run_correlation"),
  getCorrelationData: (stateFilter: string | null) =>
    invoke<CorrelationPageData>("get_correlation_data", { stateFilter }),
  reviewCorrelationCandidate: (candidateId: number, reviewState: string, note: string | null) =>
    invoke<void>("review_correlation_candidate", { candidateId, reviewState, note }),
  // M3-D 浏览器下载记录
  browserListProfiles: () => invoke<BrowserProfilesResponse>("browser_list_profiles"),
  browserAuthorizeSource: (browser: string, profile: string, historyPath: string) =>
    invoke<number>("browser_authorize_source", { browser, profile, historyPath }),
  browserRevokeSource: (sourceId: number) =>
    invoke<number>("browser_revoke_source", { sourceId }),
  browserSetSourceEnabled: (sourceId: number, enabled: boolean) =>
    invoke<boolean>("browser_set_source_enabled", { sourceId, enabled }),
  browserPurgeSourceRecords: (sourceId: number) =>
    invoke<number>("browser_purge_source_records", { sourceId }),
  browserImportSource: (sourceId: number) =>
    invoke<BrowserImportStats>("browser_import_source", { sourceId }),
  browserDownloadsList: (sourceId: number | null, limit: number) =>
    invoke<DownloadRow[]>("browser_downloads_list", { sourceId, limit }),
};

export interface DiffRunRow {
  id: number;
  volume_id: number;
  from_scan_id: number;
  to_scan_id: number;
  status: string;
  events_count: number;
  ambiguous_count: number;
  skip_reason: string | null;
  created_at: number;
}

export interface HistoryEventRow {
  id: number;
  ts: number;
  kind: string;
  volume_id: number | null;
  rel_path: string | null;
  detail: string | null;
  entity_id: number | null;
}

export interface DiffSummary {
  status: string;
  pairs_completed: number;
  pairs_skipped: number;
  events_total: number;
  discovered: number;
  modified: number;
  missing: number;
  reappeared: number;
  ambiguous: number;
  missing_suppressed: number;
  blocked_pair: [number, number] | null;
  error: string | null;
}

// ---------- M3-C：工具行为关联 ----------

/** 单条关联证据（与 Rust CorrEvidence 对应；全部字段可审计）。 */
export interface CorrEvidence {
  tool_key: string;
  /** likely = 特征文件信号；possible = 数据目录信号 */
  level: string;
  /** 工作区（卷根相对） */
  workspace_rel: string;
  signal_rel_path: string;
  /** signal_path | data_dir */
  signal_kind: string;
  signal_time_ms: number;
  file_mtime_ms: number;
  delta_ms: number;
  window_ms: number;
  /** file_discovered | file_modified */
  event_kind: string;
}

export type CorrReviewState = "pending" | "confirmed" | "denied" | "ignored";

export interface CorrCandidateRow {
  id: number;
  event_id: number;
  path_id: number | null;
  tool_id: number;
  tool_key: string;
  tool_name: string;
  volume_id: number;
  volume_root: string;
  rel_path: string;
  state: CorrReviewState;
  confidence: string;
  evidence: CorrEvidence | null;
  created_at: number;
  reviewed_at: number | null;
  review_note: string | null;
}

export interface CorrOverview {
  generation: number | null;
  last_run_at: number | null;
  total: number;
  pending: number;
  confirmed: number;
  denied: number;
  ignored: number;
}

export interface CorrSummary {
  signals: number;
  targets: number;
  attribution_count: number;
  computed: number;
  new_candidates: number;
  refreshed: number;
  stale_removed: number;
  generation: number;
}

export interface CorrelationPageData {
  overview: CorrOverview;
  candidates: CorrCandidateRow[];
}

// ---------- M3-D：浏览器下载记录 ----------

/** 浏览器档案（platform 发现结果）。 */
export interface BrowserProfile {
  browser: string;
  browser_name: string;
  profile: string;
  history_path: string;
  history_exists: boolean;
}

/** 授权源行。 */
export interface BrowserSourceRow {
  id: number;
  browser: string;
  profile: string;
  history_path: string;
  enabled: boolean;
  added_at: number;
  last_import_at: number | null;
  records: number;
  matched: number;
}

export interface BrowserProfilesResponse {
  profiles: BrowserProfile[];
  sources: BrowserSourceRow[];
}

/** 一次导入摘要。 */
export interface BrowserImportStats {
  source_id: number;
  fetched: number;
  inserted: number;
  updated: number;
  matched_total: number;
  error: string | null;
}

/** 下载记录行（最小字段集；不含浏览上下文）。 */
export interface DownloadRow {
  id: number;
  source_id: number;
  browser: string;
  profile: string;
  download_id: number;
  target_path: string;
  url: string | null;
  /** complete|in_progress|cancelled|interrupted|unknown */
  state: string;
  total_bytes: number | null;
  received_bytes: number | null;
  mime_type: string | null;
  start_time: number | null;
  end_time: number | null;
  opened: boolean;
  imported_at: number;
  volume_id: number | null;
  path_id: number | null;
  /** unmatched | matched */
  match_state: string;
}
