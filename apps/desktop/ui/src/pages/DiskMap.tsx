/** 磁盘地图（M4）：基于扫描数据库的真实卷概览——容量、目录分布、覆盖状态。
 *  语义纪律：展示的是"上次扫描快照"（含采集时间）；未扫描卷明确标注"未扫描"，
 *  绝不显示成空目录；覆盖缺口（denied 等）随卷展示，未覆盖区域不当作完整；
 *  size 为扫描时聚合值，不代表实时。
 */
import { useCallback, useEffect, useState } from "react";
import { Card } from "../components/EmptyState";
import { api, IpcError, type VolumeOverview } from "../lib/ipc";
import { formatBytes } from "../lib/format";
import type { PageId } from "../lib/format";

function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

const SCAN_BADGE: Record<string, string> = {
  done: "border-[#6ccb5f]/40 text-[#6ccb5f]",
  partial: "border-[#f2c744]/40 text-[#f2c744]",
  running: "border-[#4cc2ff]/40 text-[#4cc2ff]",
  failed: "border-[#f26d6d]/50 text-[#f26d6d]",
  access_denied: "border-[#f26d6d]/50 text-[#f26d6d]",
  volume_offline: "border-[#f2c744]/40 text-[#f2c744]",
};

function scanLabel(st: string | null): string {
  const map: Record<string, string> = {
    done: "扫描完成",
    partial: "部分完成（存在缺口）",
    running: "扫描中",
    failed: "扫描失败",
    access_denied: "权限不足",
    volume_offline: "卷离线",
    cancelled: "已取消",
    pending: "等待扫描",
  };
  return st ? (map[st] ?? st) : "未扫描";
}

function VolumeCard(props: { o: VolumeOverview; onBrowse: (pathId: number) => void }) {
  const { o } = props;
  const used = o.total_bytes !== null && o.free_bytes !== null ? o.total_bytes - o.free_bytes : null;
  const usedPct = used !== null && o.total_bytes ? Math.min(100, (used / o.total_bytes) * 100) : null;
  return (
    <Card>
      <div className="flex flex-wrap items-center gap-2">
        <b className="text-[14px]">{o.root}</b>
        {o.label && <span className="text-[12px] text-[#9aa0a6]">{o.label}</span>}
        <span
          className={
            "rounded border px-1.5 text-[10.5px] " +
            (SCAN_BADGE[o.latest_scan_status ?? ""] ?? "border-[#35373c] text-[#6b7075]")
          }
        >
          {scanLabel(o.latest_scan_status)}
        </span>
        {o.fs_type && (
          <span className="rounded border border-[#35373c] px-1.5 text-[10.5px] text-[#6b7075]">
            {o.fs_type}
          </span>
        )}
      </div>

      {/* 容量条（卷容量来自登记时真实 GetDiskFreeSpaceExW；未知则不绘制） */}
      <div className="mt-2.5">
        {usedPct !== null ? (
          <>
            <div className="h-2.5 w-full overflow-hidden rounded bg-[#1f2023]">
              <div className="h-full bg-gradient-to-r from-[#4cc2ff] to-[#8a7bf0]" style={{ width: `${usedPct}%` }} />
            </div>
            <div className="mt-1 flex flex-wrap gap-3 text-[11.5px] text-[#9aa0a6]">
              <span>已用 {formatBytes(used)}（{usedPct.toFixed(1)}%）</span>
              <span>剩余 {formatBytes(o.free_bytes)}</span>
              <span>总容量 {formatBytes(o.total_bytes)}</span>
            </div>
          </>
        ) : (
          <div className="text-[11.5px] text-[#6b7075]">容量信息未知（卷登记时未采集）</div>
        )}
      </div>

      {/* 扫描覆盖（快照语义） */}
      <div className="mt-2.5 grid grid-cols-2 gap-2 text-[11.5px] sm:grid-cols-4">
        <div>
          <div className="text-[#6b7075]">文件</div>
          <div className="text-[#e8eaed]">{o.files.toLocaleString()}</div>
        </div>
        <div>
          <div className="text-[#6b7075]">目录</div>
          <div className="text-[#e8eaed]">{o.dirs.toLocaleString()}</div>
        </div>
        <div>
          <div className="text-[#6b7075]">覆盖缺口</div>
          <div className={o.coverage_gap_count > 0 ? "text-[#f2c744]" : "text-[#e8eaed]"}>
            {o.coverage_gap_count}
            {o.coverage_gap_count > 0 ? "（未覆盖区域不计入完整性）" : ""}
          </div>
        </div>
        <div>
          <div className="text-[#6b7075]">采集时间</div>
          <div className="text-[#e8eaed]">{fmtTime(o.latest_scan_finished_at)}</div>
        </div>
      </div>

      {/* 下钻入口 */}
      <div className="mt-2.5">
        {o.root_path_id !== null ? (
          <button
            className="rounded border border-[#4cc2ff] px-2.5 py-1 text-[11.5px] text-[#4cc2ff]"
            onClick={() => props.onBrowse(o.root_path_id as number)}
          >
            浏览该卷目录分布 →
          </button>
        ) : (
          <span className="text-[11.5px] text-[#6b7075]">
            未扫描：无目录分布数据（请在总览页执行扫描）
          </span>
        )}
      </div>
    </Card>
  );
}

export function DiskMap(props: { onNavigate: (p: PageId) => void }) {
  const [ovs, setOvs] = useState<VolumeOverview[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => {
    setErr(null);
    api
      .getVolumeOverviews()
      .then(setOvs)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const browse = (pathId: number) => {
    location.hash = `#/files?path=${pathId}`;
    props.onNavigate("files");
  };

  return (
    <div>
      <h1 className="text-[19px] font-semibold">磁盘地图</h1>
      <p className="mb-2 mt-1 text-[12px] leading-relaxed text-[#9aa0a6]">
        基于扫描数据库的卷级视图。所有数字都是「扫描时快照」——明确标注采集时间与覆盖状态；
        未扫描的卷如实标注，不显示为空目录；本页不读取文件内容、不计算哈希。
      </p>

      {err && (
        <div className="mb-4 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-4 py-3 text-[12.5px] text-[#f2c744]">
          {err}
        </div>
      )}

      {ovs === null ? (
        <div className="text-[12px] text-[#9aa0a6]">读取中…</div>
      ) : ovs.length === 0 ? (
        <Card>
          <div className="py-6 text-center text-[12px] text-[#6b7075]">
            尚无登记磁盘。启动一次扫描后，被扫描的磁盘会自动登记并出现在这里。
          </div>
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
          {ovs.map((o) => (
            <VolumeCard key={o.volume_id} o={o} onBrowse={browse} />
          ))}
        </div>
      )}
    </div>
  );
}
