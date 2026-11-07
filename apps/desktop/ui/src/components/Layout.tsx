/** 应用骨架：左侧导航 + 顶栏（版本、后端连接状态）。 */
import type { ReactNode } from "react";
import type { PageId } from "../lib/format";
import type { AppInfo } from "../lib/ipc";

const NAV_ITEMS: { id: PageId; label: string; group: string; milestone: string }[] = [
  { id: "overview", label: "总览", group: "概览", milestone: "M0" },
  { id: "diskmap", label: "磁盘地图", group: "资产", milestone: "M1" },
  { id: "files", label: "文件浏览", group: "资产", milestone: "M1" },
  { id: "tools", label: "AI 工具", group: "AI 语义", milestone: "M2" },
  { id: "projects", label: "项目资产", group: "AI 语义", milestone: "M2" },
  { id: "history", label: "历史追踪", group: "深入", milestone: "B1" },
  { id: "monitor", label: "实时监控", group: "深入", milestone: "B2" },
  { id: "reports", label: "扫描报告", group: "深入", milestone: "V0.2" },
  { id: "settings", label: "设置", group: "深入", milestone: "M0" },
];

export function Layout(props: {
  page: PageId;
  onNavigate: (p: PageId) => void;
  app: AppInfo | null;
  backendConnected: boolean;
  children: ReactNode;
}) {
  const groups = [...new Set(NAV_ITEMS.map((n) => n.group))];
  return (
    <div className="flex h-full w-full">
      {/* 侧栏 */}
      <aside className="flex w-56 shrink-0 flex-col border-r border-[#35373c] bg-[#1f2023]">
        <div className="flex items-center gap-2.5 px-4 pt-4 pb-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-[#4cc2ff] to-[#8a7bf0] text-sm font-bold text-white">
            FM
          </div>
          <div>
            <b className="text-[15px] leading-tight">FMap</b>
            <div className="text-[10px] leading-tight text-[#6b7075]">AI 文件资产地图</div>
          </div>
        </div>
        <nav className="flex-1 overflow-y-auto px-2.5 pb-3">
          {groups.map((g) => (
            <div key={g}>
              <div className="px-2.5 pt-3 pb-1 text-[11px] tracking-wide text-[#6b7075]">{g}</div>
              {NAV_ITEMS.filter((n) => n.group === g).map((n) => (
                <button
                  key={n.id}
                  onClick={() => props.onNavigate(n.id)}
                  className={
                    "mb-0.5 flex w-full items-center justify-between rounded-md px-2.5 py-2 text-left text-[13px] " +
                    (props.page === n.id
                      ? "bg-[#4cc2ff]/15 font-semibold text-[#4cc2ff]"
                      : "text-[#9aa0a6] hover:bg-[#2c2e33] hover:text-[#e8eaed]")
                  }
                >
                  <span>{n.label}</span>
                  {n.milestone !== "M0" && (
                    <span className="rounded border border-[#35373c] px-1 text-[9px] text-[#6b7075]">
                      {n.milestone}
                    </span>
                  )}
                </button>
              ))}
            </div>
          ))}
        </nav>
        <div className="border-t border-[#35373c] px-4 py-2.5 text-[11px] leading-relaxed text-[#6b7075]">
          V{props.app?.version ?? "0.1.0"} · 完全本地 · 只读
        </div>
      </aside>

      {/* 主区 */}
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center gap-3 border-b border-[#35373c] bg-[#1f2023] px-4">
          <span
            className={
              "flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] " +
              (props.backendConnected
                ? "border-[#6ccb5f]/40 text-[#6ccb5f]"
                : "border-[#f2c744]/40 text-[#f2c744]")
            }
          >
            <span
              className={
                "h-1.5 w-1.5 rounded-full " +
                (props.backendConnected ? "bg-[#6ccb5f]" : "bg-[#f2c744]")
              }
            />
            {props.backendConnected ? "后端已连接" : "后端未连接（浏览器模式）"}
          </span>
          <div className="flex-1" />
          <span className="text-[12px] text-[#9aa0a6]">
            {props.app ? `${props.app.name} v${props.app.version} · ${props.app.platform}` : "…"}
          </span>
        </header>
        <main className="flex-1 overflow-y-auto px-7 py-5">{props.children}</main>
      </div>
    </div>
  );
}
