/** 历史追踪（M3-B1 + M5 完善）：差分事件与台账的真实展示——无演示数据。
 *  语义纪律：DISCOVERED=首次观察到（≠刚创建）；MISSING=缺失待确认（≠已删除）；
 *  本页不展示、不推断任何 AI 来源信息。
 *  M5 增强：分页（每页 100）+ 按卷过滤 + 路径子串搜索 + 差分超限处置入口
 *  （提高上限自动重跑 / 人工确认放弃——处置只影响 pending_over_limit 台账行）。
 */
import { useCallback, useEffect, useState } from "react";
import { Card, DevPlaceholder } from "../components/EmptyState";
import {
  api,
  IpcError,
  type DiffRunRow,
  type HistoryEventRow,
  type VolumeOverview,
} from "../lib/ipc";
import { diffRunStatusLabel, eventKindLabel, eventNote, formatBytes } from "../lib/format";

const KIND_BADGE: Record<string, string> = {
  file_discovered: "text-[#4cc2ff] border-[#4cc2ff]/40",
  file_modified: "text-[#f2c744] border-[#f2c744]/40",
  file_missing: "text-[#f2994a] border-[#f2994a]/40",
  file_reappeared: "text-[#6ccb5f] border-[#6ccb5f]/40",
};

const PAGE_SIZE = 100;

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

/** 被阻塞差分对的处置控件（提高上限 / 确认放弃）。 */
function OverLimitActions(props: { run: DiffRunRow; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [newMax, setNewMax] = useState("100000");
  const [askAbandon, setAskAbandon] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const raise = async () => {
    setBusy(true);
    setErr(null);
    try {
      await api.diffRaiseLimit(Number(newMax));
      props.onDone();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const abandon = async () => {
    setBusy(true);
    setErr(null);
    try {
      const ok = await api.diffAbandonPair(props.run.id, "用户在历史页确认放弃该差分对");
      if (!ok) setErr("该台账行已不是待处置状态");
      setAskAbandon(false);
      props.onDone();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="mt-2 rounded border border-[#f26d6d]/40 bg-[#f26d6d]/5 px-2.5 py-2 text-[11.5px]">
      <div className="mb-1.5 text-[#f26d6d]">处置（二选一，只影响本条台账行）：</div>
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[#9aa0a6]">① 提高上限并重跑：</span>
        <input
          className="w-28 rounded border border-[#35373c] bg-[#1f2023] px-2 py-0.5 font-mono text-[11px] text-[#e8eaed] outline-none focus:border-[#4cc2ff]"
          value={newMax}
          onChange={(e) => setNewMax(e.target.value.replace(/[^\d]/g, ""))}
        />
        <button
          className="rounded border border-[#4cc2ff] px-2 py-0.5 text-[11px] text-[#4cc2ff] disabled:opacity-40"
          disabled={busy}
          onClick={raise}
        >
          提高上限
        </button>
        <span className="text-[#6b7075]">（提高后点下方「补跑差分」生效）</span>
      </div>
      <div className="mt-1.5 flex flex-wrap items-center gap-2">
        <span className="text-[#9aa0a6]">② 或</span>
        {askAbandon ? (
          <>
            <button
              className="rounded border border-[#f26d6d]/60 px-2 py-0.5 text-[11px] text-[#f26d6d] disabled:opacity-40"
              disabled={busy}
              onClick={abandon}
            >
              确认放弃（本对永不再研判）
            </button>
            <button
              className="rounded border border-[#35373c] px-2 py-0.5 text-[11px] text-[#9aa0a6]"
              onClick={() => setAskAbandon(false)}
            >
              取消
            </button>
          </>
        ) : (
          <button
            className="rounded border border-[#f26d6d]/50 px-2 py-0.5 text-[11px] text-[#f26d6d] disabled:opacity-40"
            disabled={busy}
            onClick={() => setAskAbandon(true)}
          >
            确认放弃这对差分
          </button>
        )}
      </div>
      {err && <div className="mt-1.5 text-[#f26d6d]">{err}</div>}
    </div>
  );
}

export function History() {
  const [events, setEvents] = useState<HistoryEventRow[] | null>(null);
  const [total, setTotal] = useState(0);
  const [runs, setRuns] = useState<DiffRunRow[] | null>(null);
  const [volumes, setVolumes] = useState<VolumeOverview[]>([]);
  const [page, setPage] = useState(0);
  const [volFilter, setVolFilter] = useState<number | null>(null);
  const [search, setSearch] = useState("");
  const [applied, setApplied] = useState<{ vol: number | null; q: string }>({ vol: null, q: "" });
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    (pageNo: number, vol: number | null, q: string) => {
      setErr(null);
      api
        .getHistoryEvents(vol, q || null, PAGE_SIZE, pageNo * PAGE_SIZE)
        .then((p) => {
          setEvents(p.rows);
          setTotal(p.total);
        })
        .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
      api
        .getDiffRuns(null)
        .then(setRuns)
        .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
    },
    []
  );

  useEffect(() => {
    load(page, applied.vol, applied.q);
  }, [load, page, applied]);

  // 卷下拉（磁盘地图数据源复用；失败静默——过滤是增强功能）
  useEffect(() => {
    api
      .getVolumeOverviews()
      .then(setVolumes)
      .catch(() => undefined);
  }, []);

  const applyFilters = () => {
    setPage(0);
    setApplied({ vol: volFilter, q: search.trim() });
  };

  const rerunDiff = async () => {
    if (!runs?.length) return;
    setBusy(true);
    try {
      // 对台账中出现的每个卷手动补跑一次差分（pending_over_limit 对会自动重试）
      const vids = [...new Set(runs.map((r) => r.volume_id))];
      for (const v of vids) {
        await api.runDiffNow(v);
      }
      load(page, applied.vol, applied.q);
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE));

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
            <span className="text-[11px] text-[#6b7075]">用于超限处置后重跑 / 手动补齐</span>
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
                  <OverLimitActions run={r} onDone={() => load(page, applied.vol, applied.q)} />
                )}
              </div>
            ))}
          </div>
        </>
      )}

      {/* 过滤与搜索 */}
      <h2 className="mb-2 mt-5 text-[14px] font-semibold">文件事件（最新在前）</h2>
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <select
          className="rounded border border-[#35373c] bg-[#1f2023] px-2 py-1.5 text-[12px] text-[#e8eaed] outline-none focus:border-[#4cc2ff]"
          value={volFilter ?? ""}
          onChange={(e) => {
            const v = e.target.value === "" ? null : Number(e.target.value);
            setVolFilter(v);
            setPage(0);
            setApplied({ vol: v, q: search.trim() });
          }}
        >
          <option value="">全部卷</option>
          {volumes.map((v) => (
            <option key={v.volume_id} value={v.volume_id}>
              {v.root}（{v.latest_scan_status ?? "未扫描"}）
            </option>
          ))}
        </select>
        <input
          className="w-64 rounded border border-[#35373c] bg-[#1f2023] px-2.5 py-1.5 text-[12px] text-[#e8eaed] outline-none focus:border-[#4cc2ff]"
          placeholder="按路径子串搜索（如 .claude 或 章节）"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && applyFilters()}
        />
        <button
          className="rounded border border-[#4cc2ff] px-3 py-1.5 text-[12px] text-[#4cc2ff]"
          onClick={applyFilters}
        >
          搜索
        </button>
        {(applied.q || applied.vol !== null) && (
          <button
            className="rounded border border-[#35373c] px-3 py-1.5 text-[12px] text-[#9aa0a6]"
            onClick={() => {
              setSearch("");
              setVolFilter(null);
              setPage(0);
              setApplied({ vol: null, q: "" });
            }}
          >
            清除过滤
          </button>
        )}
        <span className="text-[11px] text-[#6b7075]">
          共 {total.toLocaleString()} 条{applied.q ? `（匹配「${applied.q}」）` : ""}
        </span>
      </div>

      {events === null ? (
        <div className="text-[12px] text-[#9aa0a6]">读取中…</div>
      ) : events.length === 0 ? (
        <div className="rounded-md border border-dashed border-[#35373c] bg-[#1f2023] px-6 py-10 text-center">
          <div className="text-[14px] text-[#9aa0a6]">当前条件下没有事件</div>
          <div className="mt-1.5 text-[12px] text-[#6b7075]">
            事件来自相邻两次有效扫描的差分。请在总览页执行至少两次扫描，或调整过滤条件。
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

      {/* 分页 */}
      {total > PAGE_SIZE && (
        <div className="mt-2 flex items-center justify-center gap-3 text-[12px]">
          <button
            className="rounded border border-[#35373c] px-3 py-1 text-[#9aa0a6] disabled:opacity-40"
            disabled={page === 0}
            onClick={() => setPage((p) => Math.max(0, p - 1))}
          >
            上一页
          </button>
          <span className="text-[#9aa0a6]">
            第 {page + 1} / {totalPages} 页
          </span>
          <button
            className="rounded border border-[#35373c] px-3 py-1 text-[#9aa0a6] disabled:opacity-40"
            disabled={page + 1 >= totalPages}
            onClick={() => setPage((p) => p + 1)}
          >
            下一页
          </button>
        </div>
      )}

      <div className="mt-3 text-[11px] leading-relaxed text-[#6b7075]">
        只读展示：事件来自本地数据库的差分研判；不读取文件内容、不判断文件来源、
        不上传任何数据。扫描覆盖不完整时（权限/排除/截断），缺失判断会保守跳过并
        在台账中注明。
      </div>
    </div>
  );
}
