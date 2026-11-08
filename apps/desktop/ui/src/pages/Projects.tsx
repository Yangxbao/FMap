/** 项目资产（M5）：以「AI 工具工作区」为单元聚合的真实资产视图。
 *  语义纪律：工作区 = 数据目录/特征文件的父目录（归属锚点推导）；
 *  文件数/大小来自扫描快照聚合；活动数来自差分事件。全部只读展示，
 *  不推断任何来源行为。
 */
import { useCallback, useEffect, useState } from "react";
import { Card } from "../components/EmptyState";
import { EmptyWithAction, PageHeader, StatStrip } from "../components/PageHeader";
import { api, IpcError, type WorkspaceRow } from "../lib/ipc";
import { formatBytes } from "../lib/format";
import type { PageId } from "../lib/format";

function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

const ANCHOR_LABEL: Record<string, string> = {
  dir: "数据目录锚点",
  signal: "特征文件锚点",
};

export function Projects(props: { onNavigate: (p: PageId) => void }) {
  const [ws, setWs] = useState<WorkspaceRow[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => {
    setErr(null);
    api
      .getProjects()
      .then(setWs)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const totalFiles = ws?.reduce((s, w) => s + w.files, 0) ?? 0;
  const totalSize = ws?.reduce((s, w) => s + w.total_size, 0) ?? 0;
  const toolKeys = new Set(ws?.flatMap((w) => w.tools.map((t) => t.tool_key)) ?? []);

  const browse = (pathId: number) => {
    location.hash = `#/files?path=${pathId}`;
    props.onNavigate("files");
  };

  return (
    <div>
      <PageHeader
        title="项目资产"
        subtitle="以「AI 工具工作区」为单元聚合：工作区 = 数据目录或特征文件所在的目录（由归属锚点推导）。
        文件数与大小来自扫描快照聚合，活动数来自差分事件——全部只读展示，不推断来源行为。"
        actions={
          <button
            className="rounded border border-[#4cc2ff] px-3 py-1.5 text-[12px] text-[#4cc2ff] transition-colors hover:bg-[#4cc2ff]/10"
            onClick={load}
          >
            刷新
          </button>
        }
      />

      {err && (
        <div className="mb-4 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-4 py-3 text-[12.5px] text-[#f2c744]">
          {err}
        </div>
      )}

      {ws !== null && ws.length > 0 && (
        <StatStrip
          items={[
            { label: "工作区", value: ws.length, accent: "text-[#4cc2ff]" },
            { label: "涉及工具", value: toolKeys.size },
            { label: "追踪文件", value: totalFiles.toLocaleString() },
            { label: "快照体量", value: formatBytes(totalSize) },
            {
              label: "近期活动",
              value: ws.reduce((s, w) => s + w.events, 0).toLocaleString(),
              title: "差分事件总数（工作区内）",
            },
          ]}
        />
      )}

      {ws === null ? (
        <div className="text-[12px] text-[#9aa0a6]">读取中…</div>
      ) : ws.length === 0 ? (
        <EmptyWithAction
          title="尚无工作区"
          hint="工作区由「AI 工具」页的归属分析推导（需要 verified/likely 的目录或特征文件锚点）。
                请先在总览页扫描，再到 AI 工具页运行归属分析。"
          actionLabel="去 AI 工具页运行归属分析 →"
          actionHash="#/tools"
        />
      ) : (
        <div className="space-y-2">
          {ws.map((w) => (
            <Card key={w.path_id}>
              <div className="flex flex-wrap items-center gap-2">
                <b className="text-[13.5px]">{w.volume_root}{w.rel_path}</b>
                {w.tools.map((t) => (
                  <span
                    key={t.tool_key}
                    className="rounded border border-[#8a7bf0]/40 bg-[#8a7bf0]/10 px-1.5 py-0.5 text-[10.5px] text-[#b9aef5]"
                    title={`${t.tool_name} · ${ANCHOR_LABEL[t.anchor] ?? t.anchor}`}
                  >
                    {t.tool_name || t.tool_key}
                  </span>
                ))}
              </div>
              <div className="mt-2 grid grid-cols-2 gap-2 text-[11.5px] sm:grid-cols-4">
                <div>
                  <div className="text-[#6b7075]">追踪文件</div>
                  <div className="text-[#e8eaed]">{w.files.toLocaleString()}</div>
                </div>
                <div>
                  <div className="text-[#6b7075]">快照体量</div>
                  <div className="text-[#e8eaed]">{formatBytes(w.total_size)}</div>
                </div>
                <div>
                  <div className="text-[#6b7075]">差分事件</div>
                  <div className={w.events > 0 ? "text-[#f2c744]" : "text-[#e8eaed]"}>
                    {w.events.toLocaleString()}
                  </div>
                </div>
                <div>
                  <div className="text-[#6b7075]">最近活动</div>
                  <div className="text-[#e8eaed]">{fmtTime(w.last_event_ts)}</div>
                </div>
              </div>
              <div className="mt-2.5">
                <button
                  className="rounded border border-[#4cc2ff] px-2.5 py-1 text-[11.5px] text-[#4cc2ff] transition-colors hover:bg-[#4cc2ff]/10"
                  onClick={() => browse(w.path_id)}
                >
                  在文件浏览中打开 →
                </button>
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
