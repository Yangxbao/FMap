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
  error: string | null;
}

/** 后端命令的唯一访问点（新增命令在此补充）。 */
export const api = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  getSystemStatus: () => invoke<SystemStatus>("get_system_status"),
  listVolumes: () => invoke<VolumeInfo[]>("list_volumes"),
  getDbStats: () => invoke<DbStats>("get_db_stats"),
  scanStart: (roots: string[]) => invoke<ScanProgress>("scan_start", { roots }),
  scanStatus: (scanId: number) => invoke<ScanProgress>("scan_status", { scanId }),
  scanCancel: (scanId: number) => invoke<boolean>("scan_cancel", { scanId }),
};
