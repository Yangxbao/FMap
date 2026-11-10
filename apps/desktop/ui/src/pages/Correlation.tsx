/** 行为关联（M3-C）：工具活动信号 × 文件观察事件的时间+路径关联候选。
 *  语义纪律（红线）：
 *  - 关联 ≠ 来源：候选只表述"文件最后修改时间与工具活动观察存在时间关联"，
 *    绝不表述"由工具创建/修改"——来源行为只能在 AI 工具页由用户确认；
 *  - 每条候选带完整证据（工作区/信号/时间窗/差值），可审查、可否认、可忽略、可撤回；
 *  - 证据不足不产候选（无候选 = 未知，诚实展示）。
 */
import { useCallback, useEffect, useState } from "react";
import { Card } from "../components/EmptyState";
import { PageHeader } from "../components/PageHeader";
import { api, IpcError, type CorrCandidateRow, type CorrelationPageData, type VolumeOverview } from "../lib/ipc";
import {
  corrConfidenceLabel,
  corrSignalKindLabel,
  eventKindLabel,
  fmtDelta,
  reviewStateLabel,
} from "../lib/format";

function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

const STATE_BADGE: Record<string, string> = {
  pending: "border-[#f2c744]/40 text-[#f2c744]",
  confirmed: "border-[#6ccb5f]/40 text-[#6ccb5f]",
  denied: "border-[#f26d6d]/50 text-[#f26d6d]",
  ignored: "border-[#35373c] text-[#6b7075]",
};

const FILTERS: { key: string; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "pending", label: "待审查" },
  { key: "confirmed", label: "已确认关联" },
  { key: "denied", label: "已否认" },
  { key: "ignored", label: "已忽略" },
];

function StatBox(props: { label: string; value: number | string; accent?: string }) {
  return (
    <div className="flex-1 rounded-md border border-[#35373c] bg-[#24262a] px-3 py-2.5">
      <div className="text-[11px] text-[#6b7075]">{props.label}</div>
      <div className={"text-[17px] font-semibold " + (props.accent ?? "text-[#e8eaed]")}>
        {props.value}
      </div>
    </div>
  );
}

/** 单条候选行：证据明细 + 审查动作。 */
function CandidateItem(props: {
  c: CorrCandidateRow;
  busy: boolean;
  onReview: (id: number, state: string) => void;
}) {
  const { c } = props;
  const ev = c.evidence;
  return (
    <div className="border-b border-[#35373c] px-1 py-3 text-[12px] last:border-0">
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={
            "rounded border px-1.5 text-[10.5px] " + (STATE_BADGE[c.state] ?? "border-[#35373c] text-[#9aa0a6]")
          }
        >
          {reviewStateLabel(c.state)}
        </span>
        <b>{c.tool_name || c.tool_key}</b>
        <span className="break-all text-[#e8eaed]">
          {c.volume_root}
          {c.rel_path}
        </span>
        {ev && (
          <span
            className={
              "rounded border px-1.5 text-[10.5px] " +
              (ev.level === "likely"
                ? "border-[#4cc2ff]/40 text-[#4cc2ff]"
                : "border-[#35373c] text-[#9aa0a6]")
            }
          >
            {corrConfidenceLabel(c.confidence)}
          </span>
        )}
        <span className="text-[#6b7075]">事件 #{c.event_id}</span>
      </div>

      {ev ? (
        <div className="mt-1.5 space-y-0.5 rounded bg-[#1f2023] px-3 py-2 text-[11.5px] leading-relaxed text-[#9aa0a6]">
          <div>
            <b className="text-[#e8eaed]">证据：</b>该文件的最后修改时间与{" "}
            <b className="text-[#e8eaed]">{c.tool_name || c.tool_key}</b> 在工作区{" "}
            <code className="text-[#4cc2ff]">
              {c.volume_root}
              {ev.workspace_rel}
            </code>{" "}
            的活动观察存在<b>时间关联</b>（{eventKindLabel(ev.event_kind)}）。
          </div>
          <div>
            信号：{corrSignalKindLabel(ev.signal_kind)}{" "}
            <code className="text-[#4cc2ff]">
              {c.volume_root}
              {ev.signal_rel_path}
            </code>{" "}
            · 信号时间 {fmtTime(ev.signal_time_ms)} · 文件最后修改 {fmtTime(ev.file_mtime_ms)}
          </div>
          <div>
            时间差 {fmtDelta(ev.delta_ms)}（窗口 ±{fmtDelta(ev.window_ms)}）。
            这是<b>观察到的元数据时间关联</b>，不代表文件由该工具创建或修改。
          </div>
        </div>
      ) : (
        <div className="mt-1.5 text-[11px] text-[#f26d6d]">证据数据缺失（异常候选，请忽略或否认）</div>
      )}

      {c.review_note && (
        <div className="mt-1 text-[11px] text-[#6b7075]">审查备注：{c.review_note}</div>
      )}
      {c.reviewed_at && (
        <div className="mt-0.5 text-[11px] text-[#6b7075]">审查于 {fmtTime(c.reviewed_at)}</div>
      )}

      <div className="mt-2 flex flex-wrap gap-2">
        {c.state !== "confirmed" && (
          <button
            className="rounded border border-[#6ccb5f]/60 px-2.5 py-1 text-[11px] text-[#6ccb5f] disabled:opacity-40"
            disabled={props.busy}
            onClick={() => props.onReview(c.id, "confirmed")}
          >
            确认关联
          </button>
        )}
        {c.state !== "denied" && (
          <button
            className="rounded border border-[#f26d6d]/50 px-2.5 py-1 text-[11px] text-[#f26d6d] disabled:opacity-40"
            disabled={props.busy}
            onClick={() => props.onReview(c.id, "denied")}
          >
            否认
          </button>
        )}
        {c.state !== "ignored" && (
          <button
            className="rounded border border-[#35373c] px-2.5 py-1 text-[11px] text-[#9aa0a6] disabled:opacity-40"
            disabled={props.busy}
            onClick={() => props.onReview(c.id, "ignored")}
          >
            忽略
          </button>
        )}
        {c.state !== "pending" && (
          <button
            className="rounded border border-[#f2c744]/40 px-2.5 py-1 text-[11px] text-[#f2c744] disabled:opacity-40"
            disabled={props.busy}
            onClick={() => props.onReview(c.id, "pending")}
          >
            撤回待审
          </button>
        )}
      </div>
    </div>
  );
}

export function Correlation() {
  const [data, setData] = useState<CorrelationPageData | null>(null);
  const [filter, setFilter] = useState<string>("all");
  const [volFilter, setVolFilter] = useState<number | null>(null);
  const [page, setPage] = useState(0);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [recalcNote, setRecalcNote] = useState<string | null>(null);
  const [volumes, setVolumes] = useState<VolumeOverview[]>([]);
  const PAGE = 50;

  const load = useCallback(
    (f: string, v: number | null, pageNo: number) => {
      setErr(null);
      api
        .getCorrelationData(f === "all" ? null : f, v, PAGE, pageNo * PAGE)
        .then(setData)
        .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
    },
    []
  );

  useEffect(() => {
    load(filter, volFilter, page);
  }, [load, filter, volFilter, page]);

  // 卷下拉数据源（失败静默——过滤是增强功能）
  useEffect(() => {
    api
      .getVolumeOverviews()
      .then(setVolumes)
      .catch(() => undefined);
  }, []);

  const recalc = async () => {
    setBusy(true);
    setRecalcNote(null);
    setErr(null);
    try {
      const s = await api.runCorrelation();
      setRecalcNote(
        `重算完成：信号 ${s.signals}（归属行 ${s.attribution_count}）× 目标事件 ${s.targets} → ` +
          `候选 ${s.computed}（新增 ${s.new_candidates} / 刷新 ${s.refreshed} / 清理过期 ${s.stale_removed}）` +
          `，代号 #${s.generation}。已审查的候选永久保留。`
      );
      load(filter, volFilter, page);
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const review = async (id: number, state: string) => {
    setBusy(true);
    setErr(null);
    try {
      await api.reviewCorrelationCandidate(id, state, null);
      load(filter, volFilter, page);
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const ov = data?.overview;

  return (
    <div>
      <PageHeader
        title="行为关联（AI 工具 × 文件观察）"
        subtitle={
          <>  
          候选 = 「文件位于工具活动工作区内 <b>且</b> 其最后修改时间落在工具活动观察的时间窗口内」。
          这是<b>可审查的时间关联</b>，不是来源判断——时间共现与路径位置都不足以认定
          「由 AI 创建/修改」。来源行为的唯一入口是 AI 工具页的来源确认。
          证据不足时不产生候选。
        
          </>
        }
      />

      {err && (
        <div className="mb-4 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-4 py-3 text-[12.5px] text-[#f2c744]">
          {err}
        </div>
      )}

      {/* 总览 */}
      <div className="mb-3 flex flex-wrap gap-2">
        <StatBox label="待审查" value={ov?.pending ?? "—"} accent="text-[#f2c744]" />
        <StatBox label="已确认关联" value={ov?.confirmed ?? "—"} accent="text-[#6ccb5f]" />
        <StatBox label="已否认" value={ov?.denied ?? "—"} accent="text-[#f26d6d]" />
        <StatBox label="已忽略" value={ov?.ignored ?? "—"} />
        <StatBox label="重算代号" value={ov?.generation ?? "—"} />
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-2.5">
        <button
          className="rounded border border-[#4cc2ff] px-3 py-1 text-[12px] text-[#4cc2ff] disabled:opacity-40"
          disabled={busy}
          onClick={recalc}
        >
          {busy ? "计算中…" : "重新计算关联"}
        </button>
        <span className="text-[11px] text-[#6b7075]">
          基于归属分析结果与差分事件；请先在 AI 工具页运行归属分析、确保有差分事件
          {ov?.last_run_at ? ` · 上次重算 ${fmtTime(ov.last_run_at)}` : ""}
        </span>
      </div>

      {recalcNote && (
        <div className="mb-4 rounded-md border border-[#4cc2ff]/40 bg-[#4cc2ff]/10 px-4 py-3 text-[12px] text-[#4cc2ff]">
          {recalcNote}
        </div>
      )}

      {/* 状态过滤 + 卷过滤 */}
      <div className="mb-2 flex flex-wrap items-center gap-1.5">
        {FILTERS.map((f) => (
          <button
            key={f.key}
            className={
              "rounded border px-2.5 py-1 text-[11.5px] " +
              (filter === f.key
                ? "border-[#4cc2ff] bg-[#4cc2ff]/10 text-[#4cc2ff]"
                : "border-[#35373c] text-[#9aa0a6] hover:text-[#e8eaed]")
            }
            onClick={() => { setFilter(f.key); setPage(0); }}
          >
            {f.label}
            {f.key !== "all" && ov
              ? ` (${f.key === "pending" ? ov.pending : f.key === "confirmed" ? ov.confirmed : f.key === "denied" ? ov.denied : ov.ignored})`
              : ""}
          </button>
        ))}
        <select
          className="ml-auto rounded border border-[#35373c] bg-[#1f2023] px-2 py-1 text-[11.5px] text-[#e8eaed] outline-none focus:border-[#4cc2ff]"
          value={volFilter ?? ""}
          onChange={(e) => {
            setVolFilter(e.target.value === "" ? null : Number(e.target.value));
            setPage(0);
          }}
        >
          <option value="">全部卷</option>
          {(volumes ?? []).map((v) => (
            <option key={v.volume_id} value={v.volume_id}>
              {v.root}
            </option>
          ))}
        </select>
      </div>

      {/* 候选列表 */}
      {data === null ? (
        <div className="text-[12px] text-[#9aa0a6]">读取中…</div>
      ) : data.candidates.length === 0 ? (
        <Card>
          <div className="py-6 text-center">
            <div className="text-[14px] text-[#9aa0a6]">当前过滤条件下没有候选</div>
            <div className="mt-1.5 text-[12px] leading-relaxed text-[#6b7075]">
              候选需要同时满足：① 已运行归属分析（产生工具数据目录/特征文件信号）；
              ② 存在差分观察事件；③ 文件 mtime 与信号时间在窗口内且位于同一工作区。
              没有候选 = 没有足够证据，这是诚实状态而非错误。
            </div>
          </div>
        </Card>
      ) : (
        <>
          <Card>
            {data.candidates.map((c) => (
              <CandidateItem key={c.id} c={c} busy={busy} onReview={review} />
            ))}
          </Card>
          {/* 分页 */}
          {data.total > PAGE && (
            <div className="mt-2 flex items-center justify-center gap-3 text-[12px]">
              <button
                className="rounded border border-[#35373c] px-3 py-1 text-[#9aa0a6] disabled:opacity-40"
                disabled={page === 0}
                onClick={() => setPage((p) => Math.max(0, p - 1))}
              >
                上一页
              </button>
              <span className="text-[#9aa0a6]">
                共 {data.total} 条 · 第 {page + 1} / {Math.ceil(data.total / PAGE)} 页
              </span>
              <button
                className="rounded border border-[#35373c] px-3 py-1 text-[#9aa0a6] disabled:opacity-40"
                disabled={(page + 1) * PAGE >= data.total}
                onClick={() => setPage((p) => p + 1)}
              >
                下一页
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
