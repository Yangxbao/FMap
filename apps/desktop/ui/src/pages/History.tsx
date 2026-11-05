/** 历史追踪（M3-B1）：差分事件与台账的真实展示——无演示数据。
 *  语义纪律：DISCOVERED=首次观察到（≠刚创建）；MISSING=缺失待确认（≠已删除）；
 *  本页不展示、不推断任何 AI 来源信息。
 */
import { useCallback, useEffect, useState } from "react";
import { Card, DevPlaceholder } from "../components/EmptyState";
import { api, IpcError, type DiffRunRow, type HistoryEventRow } from "../lib/ipc";
import { diffRunStatusLabel, eventKindLabel, eventNote, formatBytes } from "../lib/format";

const KIND_BADGE: Record<string, string> = {
  file_discovered: "text-[#4cc2ff] border-[#4cc2ff]/40",
  file_modified: "text-[#f2c744] border-[#f2c744]/40",
  file_missing: "text-[#f2994a] border-[#f2994a]/40",
  file_reappeared: "text-[#6ccb5f] border-[#6ccb5f]/40",
};

function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

/** 从 detail JSON 提取 size/mtime 前后值摘要（MODIFIED/MISSING 展示原始观察值）。 */
function detailSummary(detail: string | null): string {
  if (!detail) return "";
  try {
    const o = JSON.parse(detail) as {
      from?: { size?: number; mtime?: number };
      to?: { size?: number; mtime?: number };
      last?: { size?: number; mtime?: number };
      missing_since_scan?: number;
      prev_gap_covered?: boolean;
      note?: string;
    };
    const parts: string[] = [];
    if (o.from && o.to) {
      parts.push(
        `size ${formatBytes(o.from.size ?? null)} → ${formatBytes(o.to.size ?? null)}`
      );
    }
    if (o.last) parts.push(`最后快照 ${formatBytes(o.last.size ?? null)}`);
    if (o.missing_since_scan) parts.push(`缺失起始代 #${o.missing_since_scan}`);
    if (o.prev_gap_covered) parts.push("此前可能因覆盖缺口未可见");
    if (o.note) parts.push(o.note);
    return parts.join(" · ");
  } catch {
    return "";
  }
}

export function History() {
  const [events, setEvents] = useState<HistoryEventRow[] | null>(null);
  const [runs, setRuns] = useState<DiffRunRow[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    setErr(null);
    api
      .getHistoryEvents(300)
      .then(setEvents)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
    api
      .getDiffRuns(null)
      .then(setRuns)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const rerunDiff = async () => {
    if (!runs?.length) return;
    setBusy(true);
    try {
      // 对台账中出现的每个卷手动补跑一次差分（B1 最小：同步执行）
      const vids = [...new Set(runs.map((r) => r.volume_id))];
      for (const v of vids) {
        await api.runDiffNow(v);
      }
      load();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <h1 className="text-[19px] font-semibold">历史追踪</h1>
      <p className="mb-2 mt-1 text-[12px] text-[#9aa0a6]">
        基于有效扫描代之间的差分（M3-B1）。「首次观察到」不代表文件刚刚创建；
        「缺失」不代表已删除——缺失的文件可能被移动、重命名或暂时不可见。
        本页不展示任何 AI 来源判断。
      </p>

      {err && (
        <div className="mb-4 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-4 py-3 text-[12.5px] text-[#f2c744]">
          {err}
        </div>
      )}

      {/* 差分台账 */}
      <h2 className="mb-2 mt-4 text-[14px] font-semibold">差分研判记录</h2>
      {runs === null ? (
        <div className="text-[12px] text-[#9aa0a6]">读取中…</div>
      ) : runs.length === 0 ? (
        <DevPlaceholder
          feature="尚无差分记录"
          milestone="M3-B1"
          note="对已登记磁盘执行两次扫描后，将自动产生相邻扫描代之间的差分研判。"
        />
      ) : (
        <>
          <div className="mb-2 flex items-center gap-2.5">
            <button
              className="rounded border border-[#4cc2ff] px-3 py-1 text-[12px] text-[#4cc2ff] disabled:opacity-40"
              disabled={busy}
              onClick={rerunDiff}
            >
              {busy ? "差分中…" : "补跑差分（全部卷）"}
            </button>
            <span className="text-[11px] text-[#6b7075]">用于超限处置 / 手动补齐</span>
          </div>
          <div className="space-y-1.5">
            {runs.map((r) => (
              <div
                key={r.id}
                className="rounded-md border border-[#35373c] bg-[#24262a] px-3 py-2 text-[12px]"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <b className="text-[12.5px]">卷 #{r.volume_id}</b>
                  <span
                    className={
                      "rounded border px-1.5 text-[10.5px] " +
                      (r.status === "completed"
                        ? "border-[#6ccb5f]/40 text-[#6ccb5f]"
                        : r.status === "pending_over_limit"
                          ? "border-[#f26d6d]/50 text-[#f26d6d]"
                          : "border-[#f2c744]/40 text-[#f2c744]")
                    }
                  >
                    {diffRunStatusLabel(r.status)}
                  </span>
                  <span className="text-[#9aa0a6]">
                    代 #{r.from_scan_id} → #{r.to_scan_id}
                  </span>
                  <span className="text-[#9aa0a6]">事件 {r.events_count}</span>
                  {r.ambiguous_count > 0 && (
                    <span
                      className="rounded border border-[#f2c744]/40 px-1.5 text-[10.5px] text-[#f2c744]"
                      title="这些路径的名称在大小写规范化后无法唯一区分，为避免把不同文件错误合并，已暂缓追踪"
                    >
                      路径歧义 {r.ambiguous_count}（已隔离，不合并）
                    </span>
                  )}
                  {fmtTime(r.created_at) !== "—" && (
                    <span className="text-[#6b7075]">{fmtTime(r.created_at)}</span>
                  )}
                </div>
                {r.skip_reason && (
                  <div className="mt-1 text-[11px] text-[#9aa0a6]">跳过原因：{r.skip_reason}</div>
                )}
                {r.status === "pending_over_limit" && (
                  <div className="mt-1 text-[11px] leading-relaxed text-[#f26d6d]">
                    该差分对事件量超过上限被阻塞，其后所有差分已暂停。请人工评估后
                    处理（当前版本：提高上限配置或放弃该对——处置入口在后续版本提供，
                    可通过重新扫描 + 本页补跑按钮恢复）。
                  </div>
                )}
              </div>
            ))}
          </div>
        </>
      )}

      {/* 事件时间线 */}
      <h2 className="mb-2 mt-5 text-[14px] font-semibold">文件事件（最新在前）</h2>
      {events === null ? (
        <div className="text-[12px] text-[#9aa0a6]">读取中…</div>
      ) : events.length === 0 ? (
        <div className="rounded-md border border-dashed border-[#35373c] bg-[#1f2023] px-6 py-10 text-center">
          <div className="text-[14px] text-[#9aa0a6]">尚无文件事件</div>
          <div className="mt-1.5 text-[12px] text-[#6b7075]">
            事件来自相邻两次有效扫描的差分。请在总览页执行至少两次扫描。
          </div>
        </div>
      ) : (
        <Card>
          {events.map((e) => (
            <div
              key={e.id}
              className="flex items-start gap-3 border-b border-[#35373c] px-1 py-2 text-[12px] last:border-0"
            >
              <span className="w-36 shrink-0 text-[#6b7075]">{fmtTime(e.ts)}</span>
              <span
                className={
                  "w-24 shrink-0 rounded border px-1.5 text-center text-[10.5px] " +
                  (KIND_BADGE[e.kind] ?? "border-[#35373c] text-[#9aa0a6]")
                }
              >
                {eventKindLabel(e.kind)}
              </span>
              <span className="w-14 shrink-0 text-[#6b7075]">卷 #{e.volume_id ?? "?"}</span>
              <div className="min-w-0 flex-1">
                <div className="truncate font-mono text-[11.5px]" title={e.rel_path ?? ""}>
                  {e.rel_path ?? "(路径未知)"}
                </div>
                {(detailSummary(e.detail) || eventNote(e.detail)) && (
                  <div className="mt-0.5 text-[11px] text-[#6b7075]">
                    {detailSummary(e.detail) || eventNote(e.detail)}
                  </div>
                )}
              </div>
            </div>
          ))}
        </Card>
      )}

      <div className="mt-3 text-[11px] leading-relaxed text-[#6b7075]">
        只读展示：事件来自本地数据库的差分研判；不读取文件内容、不判断文件来源、
        不上传任何数据。扫描覆盖不完整时（权限/排除/截断），缺失判断会保守跳过并
        在台账中注明。
      </div>
    </div>
  );
}
