/** AI 工具识别中心（M2 归属 + M3-A 来源确认）：真实归属数据展示，无演示数据。
 *  数据源：run_attribution / get_tools_overview / get_tool_detail（本地 SQLite）。
 *  语义红线：工具关联 ≠ 文件来源。来源行为只能由用户确认（M3-A），
 *  未确认一律显示「未知」。 */
import { useCallback, useEffect, useState } from "react";
import { api, IpcError, type PathSourceDetail, type SourceBehavior } from "../lib/ipc";
import { eventKindLabel, sourceActionLabel, sourceBehaviorLabel } from "../lib/format";

type Overview = {
  key: string;
  name: string;
  kind: string;
  color: string;
  files: number;
  dirs: number;
  verified: number;
  likely: number;
  possible: number;
  last_seen: number | null;
  conflicts: number;
  rules_version: string;
  unknown_files: number;
};

type Detail = {
  key: string;
  name: string;
  kind: string;
  top_paths: { path_id: number; full_path: string; is_dir: boolean; size: number; confidence: string }[];
  rules: {
    id: number;
    pattern: string;
    kind: string;
    confidence: string;
    enabled: boolean;
    source: string;
    evidence: string | null;
  }[];
  conflicts: number;
};

function fmtBytes(n: number): string {
  if (n >= 1073741824) return (n / 1073741824).toFixed(1) + " GB";
  if (n >= 1048576) return (n / 1048576).toFixed(1) + " MB";
  if (n >= 1024) return (n / 1024).toFixed(1) + " KB";
  return n + " B";
}

const LEVEL_BADGE: Record<string, string> = {
  verified: "text-[#6ccb5f] border-[#6ccb5f]/40",
  likely: "text-[#f2c744] border-[#f2c744]/40",
  possible: "text-[#9aa0a6] border-[#35373c]",
};

const CONFIRMABLE: SourceBehavior[] = [
  "ai_created",
  "ai_modified",
  "ai_downloaded",
  "user_created",
  "user_modified",
];

function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

/** 来源确认面板（M3-A）：身份信息 + 有效结论 + 确认历史 + 事件时间线 + 确认表单。 */
function SourcePanel(props: { pathId: number; onDone: () => void }) {
  const [detail, setDetail] = useState<PathSourceDetail | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [behavior, setBehavior] = useState<SourceBehavior>("ai_created");
  const [note, setNote] = useState("");

  const load = useCallback(() => {
    api
      .getPathSourceDetail(props.pathId)
      .then(setDetail)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
  }, [props.pathId]);

  useEffect(() => {
    load();
  }, [load]);

  const submit = async () => {
    setBusy(true);
    setErr(null);
    try {
      await api.confirmSource(props.pathId, behavior, note.trim() ? note.trim() : null);
      setNote("");
      load();
      props.onDone();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async () => {
    setBusy(true);
    setErr(null);
    try {
      await api.revokeSource(props.pathId, note.trim() ? note.trim() : null);
      setNote("");
      load();
      props.onDone();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  if (err && !detail) return <div className="px-3 py-2 text-[12px] text-[#f26d6d]">{err}</div>;
  if (!detail) return <div className="px-3 py-2 text-[12px] text-[#9aa0a6]">加载来源信息…</div>;

  const ent = detail.entity;
  const eff = detail.effective;

  return (
    <div className="mx-1 my-1.5 rounded-md border border-[#4cc2ff]/30 bg-[#1f2023] p-3 text-[12px]">
      {/* 身份信息 */}
      <div className="mb-2 text-[11.5px] text-[#9aa0a6]">
        {ent ? (
          <>
            文件身份：<b className="text-[#e8eaed]">{ent.identity_kind === "file_id" ? `FileId ${ent.file_id}` : "弱身份（路径）"}</b>
            {" · "}首次发现 {fmtTime(ent.first_seen_at)} · 最近发现 {fmtTime(ent.last_seen_at)}
          </>
        ) : (
          <>未采集文件身份（在总览页运行身份采集后，此处才能进行来源确认）</>
        )}
      </div>

      {/* 有效结论 */}
      <div className="mb-2">
        当前来源：
        {eff && eff.behavior !== "unknown" ? (
          <span className="ml-1 rounded border border-[#6ccb5f]/40 px-1.5 text-[11px] text-[#6ccb5f]">
            {sourceBehaviorLabel(eff.behavior)}（用户{sourceActionLabel(eff.action)} · {fmtTime(eff.created_at)}）
          </span>
        ) : eff ? (
          <span className="ml-1 rounded border border-[#35373c] px-1.5 text-[11px] text-[#9aa0a6]">未知（用户已撤销确认）</span>
        ) : (
          <span className="ml-1 rounded border border-[#35373c] px-1.5 text-[11px] text-[#9aa0a6]">未知（自动分析不判定来源行为）</span>
        )}
      </div>

      {/* 确认表单 */}
      {ent && (
        <div className="mb-2 flex flex-wrap items-center gap-2">
          <select
            className="rounded border border-[#35373c] bg-[#24262a] px-2 py-1 text-[12px]"
            value={behavior}
            onChange={(e) => setBehavior(e.target.value as SourceBehavior)}
            disabled={busy}
          >
            {CONFIRMABLE.map((b) => (
              <option key={b} value={b}>{sourceBehaviorLabel(b)}</option>
            ))}
          </select>
          <input
            className="min-w-40 flex-1 rounded border border-[#35373c] bg-[#24262a] px-2 py-1 text-[12px]"
            placeholder="备注（可选）"
            value={note}
            onChange={(e) => setNote(e.target.value)}
            disabled={busy}
          />
          <button className="rounded bg-[#4cc2ff] px-3 py-1 text-[12px] font-semibold text-white disabled:opacity-40" disabled={busy} onClick={submit}>
            {eff && eff.behavior !== "unknown" ? "修正确认" : "确认来源"}
          </button>
          {eff && eff.behavior !== "unknown" && (
            <button className="rounded border border-[#f26d6d] px-3 py-1 text-[12px] text-[#f26d6d] disabled:opacity-40" disabled={busy} onClick={revoke}>
              撤销确认
            </button>
          )}
        </div>
      )}

      {err && <div className="mb-2 text-[12px] text-[#f26d6d]">{err}</div>}

      {/* 确认历史（append-only） */}
      {detail.confirmations.length > 0 && (
        <div className="mb-2">
          <div className="mb-1 text-[11px] font-semibold text-[#9aa0a6]">确认历史（保留全部修正记录）</div>
          {detail.confirmations.map((c) => (
            <div key={c.id} className="text-[11.5px] text-[#9aa0a6]">
              {fmtTime(c.created_at)} · {sourceActionLabel(c.action)} → <b>{sourceBehaviorLabel(c.behavior)}</b>
              {c.note && <> · “{c.note}”</>}
            </div>
          ))}
        </div>
      )}

      {/* 事件时间线 */}
      <div>
        <div className="mb-1 text-[11px] font-semibold text-[#9aa0a6]">事件时间线（append-only，不可被自动判断覆盖）</div>
        {detail.events.length === 0 && <div className="text-[11.5px] text-[#6b7075]">（暂无事件；发现/修改事件将由后续版本的差分追踪产生）</div>}
        {detail.events.map((e) => (
          <div key={e.id} className="text-[11.5px] text-[#9aa0a6]">
            {fmtTime(e.ts)} · {eventKindLabel(e.kind)}
            {e.evidence_id != null && <> · 证据 #{e.evidence_id}</>}
            {e.detail && e.kind === "file_id_reuse_suspected" && <> · {e.detail}</>}
          </div>
        ))}
      </div>
    </div>
  );
}

export function Tools() {
  const [overview, setOverview] = useState<Overview[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  /** 当前展开来源面板的 path_id（null = 收起） */
  const [sourcePanel, setSourcePanel] = useState<number | null>(null);

  const load = useCallback(() => {
    setError(null);
    api
      .getToolsOverview()
      .then((v) => setOverview(v as Overview[]))
      .catch((e: unknown) =>
        setError(e instanceof IpcError ? e.message : String(e))
      );
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const runAnalysis = async () => {
    setRunning(true);
    setError(null);
    try {
      await api.runAttribution();
      load();
    } catch (e: unknown) {
      setError(e instanceof IpcError ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  };

  const openDetail = async (key: string) => {
    setDetailLoading(true);
    try {
      setDetail((await api.getToolDetail(key)) as Detail);
    } catch (e: unknown) {
      setError(e instanceof IpcError ? e.message : String(e));
    } finally {
      setDetailLoading(false);
    }
  };

  const toggleRule = async (id: number, enabled: boolean) => {
    await api.setRuleEnabled(id, enabled);
    if (detail) setDetail((await api.getToolDetail(detail.key)) as Detail);
  };

  return (
    <div>
      <h1 className="text-[19px] font-semibold">AI 工具识别中心</h1>
      <p className="mb-2 mt-1 text-[12px] text-[#9aa0a6]">
        基于 AI 工具目录规则库与信号文件（仅文件名，不读取正文）的工具关联分析。
        每条结论都可展开证据链；无法判断时诚实显示未知。
      </p>
      <div className="mb-4 rounded-md border border-[#4cc2ff]/30 bg-[#4cc2ff]/5 px-4 py-2.5 text-[11.5px] leading-relaxed text-[#9aa0a6]">
        <b className="text-[#4cc2ff]">两个维度，请勿混淆：</b>
        ① <b>工具关联</b>（本页）——某路径与某个 AI 工具存在关联，以及关联的可信度；
        ② <b>文件来源行为</b>——文件是 AI 创建、AI 修改、AI 下载还是用户创建。
        位置不等于来源：.codex 内的文件是工具的配置/缓存/会话数据，不代表它是 AI 生成的代码；
        项目里有 CLAUDE.md 只说明 Claude Code 在此工作过。
        <b>来源行为全部标记为「未知」</b>，只有你手动确认才能改变。
      </div>

      {error && (
        <div className="mb-4 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-4 py-3 text-[12.5px] text-[#f2c744]">
          {error}
          {!running && (
            <button className="ml-3 underline" onClick={runAnalysis}>
              运行归属分析
            </button>
          )}
        </div>
      )}

      <div className="mb-4 flex items-center gap-3">
        <button
          className="rounded bg-[#4cc2ff] px-3.5 py-1.5 text-[12.5px] font-semibold text-white disabled:opacity-40"
          disabled={running}
          onClick={runAnalysis}
        >
          {running ? "分析中…" : "运行归属分析"}
        </button>
        {overview && (
          <span className="text-[11.5px] text-[#6b7075]">
            规则版本：{overview[0]?.rules_version ?? "—"} · 未知来源文件（全库）：
            {overview[0]?.unknown_files ?? "—"}
          </span>
        )}
      </div>

      {overview !== null && overview.length === 0 && !error && (
        <div className="rounded-md border border-dashed border-[#35373c] bg-[#1f2023] px-6 py-10 text-center">
          <div className="text-[14px] text-[#9aa0a6]">尚无归属数据</div>
          <div className="mt-1.5 text-[12px] text-[#6b7075]">
            先在总览页执行一次磁盘扫描，再点击"运行归属分析"。
          </div>
        </div>
      )}

      <div className="grid grid-cols-1 gap-3.5 xl:grid-cols-2">
        {(overview ?? []).map((t) => (
          <button
            key={t.key}
            className="rounded-md border border-[#35373c] bg-[#24262a] p-4 text-left hover:border-[#4cc2ff]/50"
            onClick={() => openDetail(t.key)}
          >
            <div className="flex items-center gap-2.5">
              <span
                className="flex h-7 w-7 items-center justify-center rounded-md text-[11px] font-bold text-white"
                style={{ background: t.color }}
              >
                {t.name.slice(0, 2)}
              </span>
              <b className="text-[14px]">{t.name}</b>
              <span className="rounded border border-[#35373c] px-1.5 text-[10px] text-[#9aa0a6]">
                {t.kind}
              </span>
              {t.conflicts > 0 && (
                <span className="rounded border border-[#f26d6d]/40 px-1.5 text-[10px] text-[#f26d6d]">
                  {t.conflicts} 处冲突
                </span>
              )}
            </div>
            <div className="mt-2.5 flex flex-wrap gap-1.5 text-[11px]">
              {t.verified > 0 && (
                <span className={"rounded border px-1.5 py-0.5 " + LEVEL_BADGE.verified}>
                  verified {t.verified}
                </span>
              )}
              {t.likely > 0 && (
                <span className={"rounded border px-1.5 py-0.5 " + LEVEL_BADGE.likely}>
                  likely {t.likely}
                </span>
              )}
              {t.possible > 0 && (
                <span className={"rounded border px-1.5 py-0.5 " + LEVEL_BADGE.possible}>
                  possible {t.possible}
                </span>
              )}
            </div>
            <div className="mt-2 grid grid-cols-3 gap-2 text-[11.5px] text-[#9aa0a6]">
              <span>文件 {t.files}</span>
              <span>目录 {t.dirs}</span>
              <span>最后发现 {fmtTime(t.last_seen)}</span>
            </div>
          </button>
        ))}
      </div>

      {detailLoading && <div className="mt-4 text-[12.5px] text-[#9aa0a6]">加载详情中…</div>}

      {detail && (
        <div className="mt-5">
          <h2 className="mb-2 text-[15px] font-semibold">
            {detail.name} · 详情
          </h2>

          <h3 className="mb-1.5 mt-3 text-[13px] font-semibold text-[#9aa0a6]">
            归属 Top 路径（按大小 · 可展开来源确认）
          </h3>
          <div className="rounded-md border border-[#35373c] bg-[#1f2023] p-1">
            {detail.top_paths.length === 0 && (
              <div className="p-3 text-[12px] text-[#6b7075]">（无）</div>
            )}
            {detail.top_paths.map((p) => (
              <div key={p.path_id}>
                <div
                  className="flex items-center gap-3 px-3 py-1.5 font-mono text-[11.5px] hover:bg-[#2c2e33]"
                >
                  <span className="w-14 shrink-0 text-[#6b7075]">{fmtBytes(p.size)}</span>
                  <span
                    className={
                      "w-16 shrink-0 rounded border px-1 text-center text-[10px] " +
                      (LEVEL_BADGE[p.confidence] ?? "")
                    }
                  >
                    {p.confidence}
                  </span>
                  <span className="truncate" title={p.full_path}>
                    {p.full_path}
                  </span>
                  <button
                    className={
                      "shrink-0 rounded border px-1.5 text-[10px] " +
                      (sourcePanel === p.path_id
                        ? "border-[#4cc2ff]/50 text-[#4cc2ff]"
                        : "border-[#35373c] text-[#9aa0a6] hover:border-[#4cc2ff]/50 hover:text-[#4cc2ff]")
                    }
                    onClick={() => setSourcePanel(sourcePanel === p.path_id ? null : p.path_id)}
                  >
                    来源
                  </button>
                </div>
                {sourcePanel === p.path_id && (
                  <SourcePanel pathId={p.path_id} onDone={() => {}} />
                )}
              </div>
            ))}
          </div>
          <div className="mt-1.5 text-[11px] text-[#6b7075]">
            「来源」= 文件来源行为的用户确认（M3-A）。工具关联（上方徽标）不等于来源：
            自动分析永远显示未知，只有你的手动确认才会改变它。
          </div>

          <h3 className="mb-1.5 mt-4 text-[13px] font-semibold text-[#9aa0a6]">
            命中规则（可启用/禁用）
          </h3>
          <div className="space-y-1.5">
            {detail.rules.map((r) => (
              <div
                key={r.id}
                className="rounded-md border border-[#35373c] bg-[#24262a] px-3 py-2 text-[12px]"
              >
                <div className="flex items-center gap-2">
                  <span className="rounded border border-[#35373c] px-1.5 text-[10px] text-[#9aa0a6]">
                    {r.kind === "dir" ? "目录规则" : "信号规则"}
                  </span>
                  <b className="font-mono text-[11.5px]">{r.pattern}</b>
                  <span className={"rounded border px-1 text-[10px] " + (LEVEL_BADGE[r.confidence] ?? "")}>
                    {r.confidence}
                  </span>
                  <span className="flex-1" />
                  <button
                    className="text-[11px] underline"
                    onClick={() => toggleRule(r.id, !r.enabled)}
                  >
                    {r.enabled ? "禁用" : "启用"}
                  </button>
                </div>
                <div className="mt-1 text-[11px] text-[#6b7075]">
                  依据：{r.evidence ?? "—"} · 来源：{r.source}
                </div>
              </div>
            ))}
          </div>

          <div className="mt-3 text-[11.5px] text-[#6b7075]">
            冲突归属：{detail.conflicts} 处（存在其他工具的同级别竞争证据时标记，不强行二选一）
          </div>
        </div>
      )}
    </div>
  );
}
