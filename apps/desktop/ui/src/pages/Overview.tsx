/** 首页总览：真实系统状态 + 磁盘列表 + 明确标记的未实现区块。 */
import { useEffect, useState } from "react";
import { Card, DevPlaceholder } from "../components/EmptyState";
import { api, IpcError, type DbStats, type SystemStatus, type VolumeInfo } from "../lib/ipc";
import { formatBytes } from "../lib/format";

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

  // 后端在线时拉取真实数据；失败时把错误如实展示（不伪造）
  useEffect(() => {
    if (!connected) return;
    api.listVolumes().then(setVolumes).catch((e: unknown) => {
      setVolumesError(e instanceof IpcError ? e.message : String(e));
    });
    api.getDbStats().then(setDbStats).catch((e: unknown) => {
      setDbStatsError(e instanceof IpcError ? e.message : String(e));
    });
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
          <div className="text-[11px] text-[#6b7075]">TraceNest 设计上拒绝管理员运行</div>
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
                <div>{s.capabilities.can_scan_metadata ? "✅" : "⏳"} 文件扫描（M1）</div>
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
