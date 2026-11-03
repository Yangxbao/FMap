/** AI 工具识别中心（M2）：真实归属数据展示，无演示数据。
 *  数据源：run_attribution / get_tools_overview / get_tool_detail（本地 SQLite）。
 */
import { useCallback, useEffect, useState } from "react";
import { api, IpcError } from "../lib/ipc";

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
  top_paths: { full_path: string; is_dir: boolean; size: number; confidence: string }[];
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
function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

const LEVEL_BADGE: Record<string, string> = {
  verified: "text-[#6ccb5f] border-[#6ccb5f]/40",
  likely: "text-[#f2c744] border-[#f2c744]/40",
  possible: "text-[#9aa0a6] border-[#35373c]",
};

export function Tools() {
  const [overview, setOverview] = useState<Overview[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

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
      <p className="mb-4 mt-1 text-[12px] text-[#9aa0a6]">
        基于 AI 工具目录规则库与信号文件（仅文件名，不读取正文）的归属判断。
        每条结论都可展开证据链；无法判断时诚实显示未知。
      </p>

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
            归属 Top 路径（按大小）
          </h3>
          <div className="rounded-md border border-[#35373c] bg-[#1f2023] p-1">
            {detail.top_paths.length === 0 && (
              <div className="p-3 text-[12px] text-[#6b7075]">（无）</div>
            )}
            {detail.top_paths.map((p, i) => (
              <div
                key={i}
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
              </div>
            ))}
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
