/** 报告中心（M5）：工具占用 / 卸载安全清单 / 空间变化——全部真实数据屏显报告。
 *  语义纪律：本页绝不删除任何文件（卸载清单仅供人工核对）；
 *  导出 = 复制报告文本到剪贴板（用户自己粘贴），FMap 永不写用户文件系统。
 */
import { useCallback, useEffect, useState } from "react";
import { Card } from "../components/EmptyState";
import { EmptyWithAction, PageHeader } from "../components/PageHeader";
import { api, IpcError, type ReportsData } from "../lib/ipc";
import { formatBytes } from "../lib/format";

function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

/** 把报告渲染为纯文本（复制导出用；与屏显同源同义）。 */
function toText(d: ReportsData): string {
  const lines: string[] = ["FMap 报告", `生成时间：${fmtTime(d.generated_at)}`, ""];
  lines.push("== 工具占用 ==");
  for (const t of d.tools) {
    lines.push(
      `${t.tool_name} (${t.tool_key})：文件 ${t.files}，${formatBytes(t.total_size)}`
    );
    for (const dir of t.data_dirs) lines.push(`  数据目录：${dir}`);
    for (const s of t.signal_files) lines.push(`  特征文件：${s}`);
  }
  lines.push("", "== 空间变化（最近差分） ==");
  for (const c of d.changes) {
    lines.push(
      `${c.volume_root}：扫描条目 ${c.files_scanned ?? "—"}，` +
        `事件 新${c.discovered}/改${c.modified}/缺${c.missing}/现${c.reappeared}` +
        `（${fmtTime(c.last_diff_at)}）`
    );
  }
  lines.push("", "本报告由 FMap 本地生成（只读元数据聚合），不含文件内容。");
  return lines.join("\n");
}

export function Reports() {
  const [data, setData] = useState<ReportsData | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);

  const load = useCallback(() => {
    setErr(null);
    api
      .getReports()
      .then(setData)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const copyReport = () => {
    if (!data) return;
    navigator.clipboard?.writeText(toText(data)).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      },
      () => undefined,
    );
  };

  return (
    <div>
      <PageHeader
        title="报告中心"
        subtitle="基于本地数据库的只读聚合报告：工具占用、卸载安全清单（仅供人工核对——本页绝不删除任何文件）、
        空间变化。导出 = 复制文本到剪贴板，FMap 永不写你的文件系统。"
        actions={
          data && data.tools.length > 0 ? (
            <button
              className="rounded border border-[#4cc2ff] px-3 py-1.5 text-[12px] text-[#4cc2ff] transition-colors hover:bg-[#4cc2ff]/10"
              onClick={copyReport}
            >
              {copied ? "已复制 ✓" : "复制报告文本"}
            </button>
          ) : undefined
        }
      />

      {err && (
        <div className="mb-4 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-4 py-3 text-[12.5px] text-[#f2c744]">
          {err}
        </div>
      )}

      {data === null ? (
        <div className="text-[12px] text-[#9aa0a6]">生成中…</div>
      ) : data.tools.length === 0 ? (
        <EmptyWithAction
          title="尚无可报告的归属数据"
          hint="报告依赖归属分析结果与差分事件。请先在总览页扫描（两次以获得事件），再到 AI 工具页运行归属分析。"
          actionLabel="去 AI 工具页 →"
          actionHash="#/tools"
        />
      ) : (
        <div className="space-y-4">
          {/* 工具占用 + 卸载清单 */}
          <Card title={`工具占用与卸载安全清单（${data.tools.length} 个工具）`}>
            <div className="space-y-2.5">
              {data.tools.map((t) => (
                <div key={t.tool_key} className="rounded-md border border-[#35373c] bg-[#1f2023] px-3 py-2.5 text-[12px]">
                  <div className="flex flex-wrap items-center gap-2.5">
                    <b className="text-[13px]">{t.tool_name}</b>
                    <span className="text-[10.5px] text-[#6b7075]">{t.tool_key}</span>
                    <span className="ml-auto text-[#e8eaed]">
                      {t.files.toLocaleString()} 文件 · {formatBytes(t.total_size)}
                    </span>
                  </div>
                  <div className="mt-1.5 flex flex-wrap gap-2">
                    <span className="text-[#9aa0a6]">
                      数据目录 {t.data_dirs.length} · 特征文件 {t.signal_files.length}
                    </span>
                    {(t.data_dirs.length > 0 || t.signal_files.length > 0) && (
                      <button
                        className="rounded border border-[#35373c] px-2 py-0.5 text-[10.5px] text-[#4cc2ff] transition-colors hover:border-[#4cc2ff]/50"
                        onClick={() => setExpanded(expanded === t.tool_key ? null : t.tool_key)}
                      >
                        {expanded === t.tool_key ? "收起" : "展开清单"}
                      </button>
                    )}
                  </div>
                  {expanded === t.tool_key && (
                    <div className="mt-2 space-y-0.5 rounded bg-[#24262a] px-2.5 py-2 font-mono text-[11px] text-[#9aa0a6]">
                      <div className="mb-1 font-sans text-[10.5px] text-[#6b7075]">
                        卸载 {t.tool_name} 前，可人工核对这些目录与文件（本页只读展示，不做任何删除）：
                      </div>
                      {t.data_dirs.map((p) => (
                        <div key={p} className="break-all text-[#e8eaed]">📁 {p}</div>
                      ))}
                      {t.signal_files.map((p) => (
                        <div key={p} className="break-all">📄 {p}</div>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </Card>

          {/* 空间变化 */}
          <Card title="空间变化（各卷最近一次差分）">
            <div className="space-y-1.5">
              {data.changes.map((c) => (
                <div
                  key={c.volume_id}
                  className="flex flex-wrap items-center gap-2.5 rounded-md border border-[#35373c] bg-[#1f2023] px-3 py-2 text-[12px]"
                >
                  <b>{c.volume_root}</b>
                  {c.files_scanned !== null ? (
                    <span className="text-[#9aa0a6]">扫描条目 {c.files_scanned.toLocaleString()}</span>
                  ) : (
                    <span className="text-[#6b7075]">未扫描</span>
                  )}
                  {c.last_diff_at !== null ? (
                    <>
                      <span className="text-[#4cc2ff]">新 {c.discovered}</span>
                      <span className="text-[#f2c744]">改 {c.modified}</span>
                      <span className="text-[#f2994a]">缺 {c.missing}</span>
                      <span className="text-[#6ccb5f]">现 {c.reappeared}</span>
                      <span className="ml-auto text-[#6b7075]">{fmtTime(c.last_diff_at)}</span>
                    </>
                  ) : (
                    <span className="text-[#6b7075]">尚无差分（需两次有效扫描）</span>
                  )}
                </div>
              ))}
            </div>
            <div className="mt-2 text-[11px] leading-relaxed text-[#6b7075]">
              事件口径：「新」=首次观察到（≠刚创建）；「缺」=缺失待确认（≠已删除）。
            </div>
          </Card>

          <div className="text-[11px] text-[#6b7075]">报告生成于 {fmtTime(data.generated_at)}</div>
        </div>
      )}
    </div>
  );
}
