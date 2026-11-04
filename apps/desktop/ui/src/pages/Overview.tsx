/** 首页总览：真实系统状态 + 磁盘列表 + 真实扫描控制（M1）+ 文件身份采集（M3-A）
 *  + 明确标记的未实现区块。 */
import { useCallback, useEffect, useRef, useState } from "react";
import { Card, DevPlaceholder } from "../components/EmptyState";
import {
  api,
  IpcError,
  type BackfillProgress,
  type DbStats,
  type IdentitySettings,
  type IdentityVolume,
  type ScanProgress,
  type SystemStatus,
  type VolumeInfo,
} from "../lib/ipc";
import { formatBytes } from "../lib/format";

const SCAN_STATUS_LABEL: Record<string, string> = {
  pending: "等待扫描", running: "扫描中", done: "已完成", cancelled: "已取消",
  access_denied: "权限不足", volume_offline: "磁盘离线", failed: "扫描失败",
};
const TERMINAL = new Set(["done", "cancelled", "access_denied", "volume_offline", "failed"]);

function ScanCard(props: { connected: boolean; volumes: VolumeInfo[] | null }) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [scan, setScan] = useState<ScanProgress | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const pollRef = useRef<number | null>(null);

  const running = scan !== null && !TERMINAL.has(scan.status);

  // 轮询进度（运行中每 1.5s；结束后停止）
  useEffect(() => {
    if (!scan || TERMINAL.has(scan.status)) return;
    pollRef.current = window.setInterval(() => {
      api.scanStatus(scan.scan_id).then((p) => {
        setScan(p);
        if (TERMINAL.has(p.status) && pollRef.current) {
          clearInterval(pollRef.current);
          pollRef.current = null;
        }
      }).catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
    }, 1500);
    return () => { if (pollRef.current) clearInterval(pollRef.current); };
  }, [scan?.scan_id, scan?.status]);

  if (!props.connected) {
    return <DevPlaceholder feature="磁盘扫描" milestone="M1（后端已实现）" note="通过 Tauri 启动后，可在此对已发现的磁盘执行真实只读扫描，进度来自后台线程。" />;
  }
  const toggle = (root: string) => {
    setSelected((s) => { const n = new Set(s); n.has(root) ? n.delete(root) : n.add(root); return n; });
  };
  const start = async () => {
    setErr(null);
    try {
      const roots = selected.size ? [...selected] : (props.volumes ?? []).map((v) => v.root);
      setScan(await api.scanStart(roots));
    } catch (e) { setErr(e instanceof IpcError ? e.message : String(e)); }
  };
  const cancel = async () => { if (scan) await api.scanCancel(scan.scan_id).catch(() => {}); };

  return (
    <Card title="磁盘扫描（M1 · 真实只读扫描）">
      <div className="mb-2.5 flex flex-wrap gap-2">
        {(props.volumes ?? []).map((v) => (
          <label key={v.root} className="flex cursor-pointer items-center gap-1.5 rounded border border-[#35373c] px-2 py-1 text-[12px]">
            <input type="checkbox" checked={selected.has(v.root)} onChange={() => toggle(v.root)} />
            <b>{v.root}</b><span className="text-[#6b7075]">{v.fs_type ?? ""}</span>
          </label>
        ))}
        {props.volumes?.length === 0 && <span className="text-[12px] text-[#6b7075]">未发现磁盘</span>}
      </div>
      <div className="flex items-center gap-2.5">
        <button className="rounded bg-[#4cc2ff] px-3.5 py-1.5 text-[12.5px] font-semibold text-white disabled:opacity-40"
          disabled={running} onClick={start}>{running ? "扫描中…" : "开始扫描"}</button>
        {running && <button className="rounded border border-[#f26d6d] px-3 py-1.5 text-[12.5px] text-[#f26d6d]" onClick={cancel}>取消</button>}
      </div>
      {err && <div className="mt-2 text-[12px] text-[#f26d6d]">{err}</div>}
      {scan && (
        <div className="mt-3 space-y-1 text-[12.5px]">
          <div>状态：<b className={scan.status === "done" ? "text-[#6ccb5f]" : scan.status === "failed" ? "text-[#f26d6d]" : ""}>
            {SCAN_STATUS_LABEL[scan.status] ?? scan.status}</b>
            {scan.status === "done" && scan.denied === 0 ? "（全部成功）" : ""}
            {scan.denied > 0 && TERMINAL.has(scan.status) ? "（部分完成：存在无权访问目录）" : ""}
          </div>
          <div className="text-[#9aa0a6]">文件 {scan.files} · 目录 {scan.dirs} · 无权/跳过 {scan.denied}</div>
          {running && scan.current_dir && <div className="truncate font-mono text-[11px] text-[#6b7075]">{scan.current_dir}</div>}
          {scan.error && <div className="text-[12px] text-[#f26d6d]">{scan.error}</div>}
        </div>
      )}
      <div className="mt-2.5 text-[11px] text-[#6b7075]">
        只读扫描：不删除、不修改任何文件；排除 $RECYCLE.BIN / System Volume Information；权限不足逐目录跳过并计数。
      </div>
    </Card>
  );
}

const BACKFILL_STATUS_LABEL: Record<string, string> = {
  idle: "未运行", running: "采集中", completed: "已完成", cancelled: "已取消", failed: "失败",
};
const BACKFILL_TERMINAL = new Set(["completed", "cancelled", "failed"]);

/** 文件身份采集卡（M3-A）：仅已授权后可用；默认关闭（授权入口在设置页）。 */
function IdentityBackfillCard(props: { connected: boolean }) {
  const [settings, setSettings] = useState<IdentitySettings | null>(null);
  const [volumes, setVolumes] = useState<IdentityVolume[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [progress, setProgress] = useState<BackfillProgress | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const pollRef = useRef<number | null>(null);

  const load = useCallback(() => {
    api.getIdentitySettings().then(setSettings).catch(() => setSettings(null));
    api.listIdentityVolumes().then((v) => {
      setVolumes(v);
      setSelected((s) => s ?? v.find((x) => x.identity_supported)?.root ?? null);
    }).catch(() => setVolumes(null));
  }, []);

  useEffect(() => {
    if (props.connected) load();
  }, [props.connected, load]);

  const running = progress?.status === "running";
  const enabled = settings?.enabled ?? false;

  // 轮询采集进度（运行中每 1.5s；结束后停止并刷新卷实体计数）
  useEffect(() => {
    if (!running) return;
    pollRef.current = window.setInterval(() => {
      api.identityBackfillStatus().then((p) => {
        setProgress(p);
        if (BACKFILL_TERMINAL.has(p.status) && pollRef.current) {
          clearInterval(pollRef.current);
          pollRef.current = null;
          load();
        }
      }).catch(() => {});
    }, 1500);
    return () => { if (pollRef.current) clearInterval(pollRef.current); };
  }, [running, load]);

  if (!props.connected) {
    return (
      <DevPlaceholder
        feature="文件身份采集"
        milestone="M3-A（后端已实现）"
        note="通过 Tauri 启动并在设置页授权后，可在此对已扫描磁盘运行身份采集（卷 GUID + NTFS FileId，只读元数据）。"
      />
    );
  }

  const start = async () => {
    if (!selected) return;
    setErr(null);
    try {
      setProgress(await api.identityBackfillStart(selected));
    } catch (e) { setErr(e instanceof IpcError ? e.message : String(e)); }
  };
  const cancel = async () => { await api.identityBackfillCancel().catch(() => {}); };

  return (
    <Card title="文件身份采集（M3-A · 只读元数据）">
      {!enabled ? (
        <div className="text-[12.5px] leading-relaxed text-[#9aa0a6]">
          身份采集<b>默认关闭</b>。请先在
          <a className="mx-1 text-[#4cc2ff] underline" href="#/settings">设置页</a>
          授权「文件身份采集」（卷 GUID + NTFS FileId；不读正文、不算哈希、不上传）。
        </div>
      ) : (
        <>
          <div className="mb-2.5 flex flex-wrap gap-2">
            {(volumes ?? []).map((v) => (
              <label key={v.volume_id} className="flex cursor-pointer items-center gap-1.5 rounded border border-[#35373c] px-2 py-1 text-[12px]">
                <input type="checkbox" checked={selected === v.root} onChange={() => setSelected(v.root)} disabled={running} />
                <b>{v.root}</b>
                <span className="text-[#6b7075]">{v.fs_type ?? "?"}{v.identity_supported ? "" : "（弱身份）"}</span>
                <span className="text-[#6b7075]">实体 {v.entity_count}</span>
              </label>
            ))}
            {(volumes ?? []).length === 0 && <span className="text-[12px] text-[#6b7075]">尚无已登记磁盘——请先执行一次扫描。</span>}
          </div>
          <div className="flex items-center gap-2.5">
            <button className="rounded bg-[#4cc2ff] px-3.5 py-1.5 text-[12.5px] font-semibold text-white disabled:opacity-40"
              disabled={running || !selected} onClick={start}>{running ? "采集中…" : "开始身份采集"}</button>
            {running && <button className="rounded border border-[#f26d6d] px-3 py-1.5 text-[12.5px] text-[#f26d6d]" onClick={cancel}>取消</button>}
          </div>
          {err && <div className="mt-2 text-[12px] text-[#f26d6d]">{err}</div>}
          {progress && (
            <div className="mt-3 space-y-1 text-[12.5px]">
              <div>状态：<b className={progress.status === "completed" ? "text-[#6ccb5f]" : progress.status === "failed" ? "text-[#f26d6d]" : ""}>
                {BACKFILL_STATUS_LABEL[progress.status] ?? progress.status}</b>
                {progress.volume_root ? `（${progress.volume_root}）` : ""}
              </div>
              <div className="text-[#9aa0a6]">
                条目 {progress.entries} · 新建实体 {progress.entities_new} · 更新 {progress.entities_updated}
                {progress.weak_identity > 0 && <> · 弱身份 {progress.weak_identity}</>}
                {progress.reuse_suspected > 0 && <> · <span className="text-[#f2c744]">重用嫌疑 {progress.reuse_suspected}</span></>}
                {progress.unmapped > 0 && <> · 未对齐 {progress.unmapped}</>}
                {progress.denied > 0 && <> · 无权/跳过 {progress.denied}</>}
              </div>
              {running && progress.current_dir && (
                <div className="truncate font-mono text-[11px] text-[#6b7075]">{progress.current_dir}</div>
              )}
              {progress.error && <div className="text-[12px] text-[#f26d6d]">{progress.error}</div>}
              {progress.status === "completed" && (
                <div className="text-[11.5px] text-[#6b7075]">
                  完成后可在「AI 工具」页对已归属路径进行来源确认（事件时间线 + 证据）。
                </div>
              )}
            </div>
          )}
          <div className="mt-2.5 text-[11px] text-[#6b7075]">
            只读元数据采集：与扫描同句柄批量取得 NTFS FileId；不读正文、不计算哈希、不上传。
          </div>
        </>
      )}
    </Card>
  );
}

function Row(props: { k: string; v: React.ReactNode; ok?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-[#35373c] py-1.5 text-[12.5px] last:border-0">
      <span className="text-[#9aa0a6]">{props.k}</span>
      <span className={(props.ok === undefined ? "" : props.ok ? "text-[#6ccb5f]" : "text-[#f26d6d]") + " text-right"}>
        {props.v}
      </span>
    </div>
  );
}

export function Overview(props: { status: SystemStatus | null; error: string | null }) {
  const [volumes, setVolumes] = useState<VolumeInfo[] | null>(null);
  const [volumesError, setVolumesError] = useState<string | null>(null);
  const [dbStats, setDbStats] = useState<DbStats | null>(null);
  const [dbStatsError, setDbStatsError] = useState<string | null>(null);

  const connected = props.status !== null;

  // 后端在线时拉取真实数据；失败时把错误如实展示（不伪造）。
  // 统计卡每 10s 自动刷新（交接遗留 #2：不再是一次性快照）。
  useEffect(() => {
    if (!connected) return;
    const refresh = () => {
      api.getDbStats().then(setDbStats).catch((e: unknown) => {
        setDbStatsError(e instanceof IpcError ? e.message : String(e));
      });
    };
    api.listVolumes().then(setVolumes).catch((e: unknown) => {
      setVolumesError(e instanceof IpcError ? e.message : String(e));
    });
    refresh();
    const timer = window.setInterval(refresh, 10_000);
    return () => clearInterval(timer);
  }, [connected]);

  const s = props.status;

  return (
    <div>
      <h1 className="text-[19px] font-semibold">总览</h1>
      <p className="mb-5 mt-1 text-[12px] text-[#9aa0a6]">
        本页展示真实系统状态。标注"开发中"的区块将在对应里程碑实现——不使用演示数据。
      </p>

      {!connected && (
        <div className="mb-4 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-4 py-3 text-[12.5px] text-[#f2c744]">
          后端未连接：{props.error ?? "当前在浏览器中直接打开 UI。"} 完整功能请通过 Tauri 启动。
        </div>
      )}

      <div className="grid grid-cols-2 gap-3.5 xl:grid-cols-4">
        <Card>
          <div className="text-[12px] text-[#9aa0a6]">应用</div>
          <div className="mt-1.5 text-[22px] font-semibold">{s ? `v${s.app.version}` : "—"}</div>
          <div className="text-[11px] text-[#6b7075]">{s ? `${s.app.name} · ${s.app.platform}` : "等待后端连接"}</div>
        </Card>
        <Card>
          <div className="text-[12px] text-[#9aa0a6]">权限模式</div>
          <div className={"mt-1.5 text-[22px] font-semibold " + (s && s.app.elevated === false ? "text-[#6ccb5f]" : s?.app.elevated === true ? "text-[#f26d6d]" : "")}>
            {s ? (s.app.elevated === false ? "普通用户" : s.app.elevated === true ? "已提权（拒绝运行）" : "未知") : "—"}
          </div>
          <div className="text-[11px] text-[#6b7075]">FMap 设计上拒绝管理员运行</div>
        </Card>
        <Card>
          <div className="text-[12px] text-[#9aa0a6]">数据库</div>
          <div className={"mt-1.5 text-[22px] font-semibold " + (s?.database_ready ? "text-[#6ccb5f]" : "")}>
            {s ? (s.database_ready ? "已就绪" : "不可用") : "—"}
          </div>
          <div className="text-[11px] text-[#6b7075]">
            {s ? (s.database_ready ? `schema v${dbStats?.schema_version ?? 1} · ${s.database_path}` : s.database_error) : "等待后端连接"}
          </div>
        </Card>
        <Card>
          <div className="text-[12px] text-[#9aa0a6]">平台能力（如实声明）</div>
          <div className="mt-2 space-y-1 text-[12px]">
            {s && (
              <>
                <div>{s.capabilities.can_list_volumes ? "✅" : "⛔"} 磁盘发现</div>
                <div>{s.capabilities.can_scan_metadata ? "✅" : "⏳"} 文件扫描（已实现）</div>
                <div>{s.capabilities.can_read_file_contents ? "✅" : "⛔"} 读取文件正文（永久关闭）</div>
                <div>{s.capabilities.can_watch ? "✅" : "⏳"} 实时监控（M5）</div>
              </>
            )}
          </div>
        </Card>
      </div>

      <h2 className="mb-2.5 mt-5 text-[14px] font-semibold">已发现的磁盘</h2>
      {!connected ? (
        <DevPlaceholder feature="磁盘发现需要连接后端" milestone="M0（后端已实现）" note="通过 Tauri 启动后，此处显示真实磁盘列表（GetLogicalDriveStringsW，普通用户权限）。" />
      ) : volumesError ? (
        <Card>
          <div className="text-[12.5px] text-[#f26d6d]">磁盘枚举失败：{volumesError}</div>
        </Card>
      ) : volumes === null ? (
        <Card><div className="text-[12.5px] text-[#9aa0a6]">读取中…</div></Card>
      ) : (
        <div className="flex flex-wrap gap-2.5">
          {volumes.map((v) => (
            <div key={v.root} className="flex items-center gap-3 rounded-md border border-[#35373c] bg-[#24262a] px-3.5 py-2.5 text-[12.5px]">
              <b className="text-[15px]">{v.root}</b>
              <span className="text-[#9aa0a6]">{v.label ?? "(无卷标)"}</span>
              <span className="text-[#6b7075]">{v.fs_type ?? "?"}</span>
              <span>
                {formatBytes(v.free_bytes)} 可用 / {formatBytes(v.total_bytes)}
              </span>
            </div>
          ))}
        </div>
      )}

      <h2 className="mb-2.5 mt-5 text-[14px] font-semibold">磁盘扫描</h2>
      <ScanCard connected={connected} volumes={volumes} />

      <h2 className="mb-2.5 mt-5 text-[14px] font-semibold">文件身份采集</h2>
      <IdentityBackfillCard connected={connected} />

      <h2 className="mb-2.5 mt-5 text-[14px] font-semibold">扫描记录</h2>
      {!connected ? (
        <DevPlaceholder feature="扫描记录" milestone="M1" />
      ) : dbStatsError ? (
        <Card><div className="text-[12.5px] text-[#f26d6d]">读取失败：{dbStatsError}</div></Card>
      ) : (
        <Card>
          {dbStats && (
            <>
              <Row k="扫描总数" v={dbStats.scans_total} />
              <Row k="进行中" v={dbStats.scans_running} />
              <Row k="已索引路径" v={dbStats.paths_total} />
              <Row k="已登记磁盘" v={dbStats.volumes} />
            </>
          )}
          {dbStats !== null && dbStats.scans_total === 0 && (
            <div className="mt-2 text-[12px] text-[#6b7075]">尚无扫描记录——扫描功能将在 M1 交付。</div>
          )}
        </Card>
      )}

      <h2 className="mb-2.5 mt-5 text-[14px] font-semibold">最近发现的资产</h2>
      <DevPlaceholder feature="AI 资产发现" milestone="M1–M2" note="依赖扫描引擎（M1）与 AI 目录规则库（M2）。" />
    </div>
  );
}
