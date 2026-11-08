/** 文件浏览（M4）：基于扫描数据库的只读文件/目录浏览 + FTS 搜索 + 详情抽屉。
 *  语义纪律：浏览的是"上次扫描快照"（每卷最新代）；不读取文件内容；
 *  不删除/移动/重命名任何文件——只提供"打开所在目录"（explorer /select）
 *  与"复制路径"两个只读动作；快照过期/未扫描如实标注。
 */
import { useCallback, useEffect, useState } from "react";
import { Card } from "../components/EmptyState";
import { PageHeader } from "../components/PageHeader";
import {
  api,
  IpcError,
  type BrowseChild,
  type BrowsePathDetail,
  type Crumb,
  type SearchHit,
} from "../lib/ipc";
import { eventKindLabel, formatBytes, sourceBehaviorLabel } from "../lib/format";

function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

const CONF_BADGE: Record<string, string> = {
  verified: "border-[#6ccb5f]/40 text-[#6ccb5f]",
  likely: "border-[#4cc2ff]/40 text-[#4cc2ff]",
  possible: "border-[#f2c744]/40 text-[#f2c744]",
};

function confLabel(c: string): string {
  const map: Record<string, string> = {
    verified: "已验证",
    likely: "较可能",
    possible: "可能",
  };
  return map[c] ?? c;
}

/** 详情抽屉（归属 + 事件 + 只读动作）。 */
function DetailDrawer(props: {
  d: BrowsePathDetail;
  onClose: () => void;
}) {
  const { d } = props;
  const [copied, setCopied] = useState(false);
  const [revealErr, setRevealErr] = useState<string | null>(null);
  return (
    <div className="fixed inset-0 z-40 flex justify-end bg-black/40" onClick={props.onClose}>
      <div
        className="h-full w-[430px] overflow-y-auto border-l border-[#35373c] bg-[#24262a] p-4"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 className="text-[14px] font-semibold">路径详情（快照）</h3>
          <button className="text-[12px] text-[#9aa0a6]" onClick={props.onClose}>
            关闭
          </button>
        </div>
        <div className="mt-2 space-y-1 text-[12px]">
          <div className="break-all text-[#e8eaed]">{d.abs_path}</div>
          <div className="text-[#9aa0a6]">
            {d.is_dir ? "目录" : "文件"} · {formatBytes(d.size)}（扫描时） · 代 #{d.scan_id}
          </div>
          <div className="text-[#9aa0a6]">mtime {fmtTime(d.mtime)} · ctime {fmtTime(d.ctime)}</div>
        </div>

        <div className="mt-3 flex gap-2">
          <button
            className="rounded border border-[#4cc2ff] px-2.5 py-1 text-[11.5px] text-[#4cc2ff]"
            onClick={() => {
              navigator.clipboard?.writeText(d.abs_path).then(
                () => {
                  setCopied(true);
                  setTimeout(() => setCopied(false), 1500);
                },
                () => undefined,
              );
            }}
          >
            {copied ? "已复制 ✓" : "复制路径"}
          </button>
          <button
            className="rounded border border-[#35373c] px-2.5 py-1 text-[11.5px] text-[#9aa0a6]"
            onClick={() => {
              setRevealErr(null);
              api.revealInExplorer(d.abs_path).catch((e: unknown) =>
                setRevealErr(e instanceof IpcError ? e.message : String(e)),
              );
            }}
          >
            打开所在目录
          </button>
        </div>
        {revealErr && (
          <div className="mt-2 text-[11px] text-[#f26d6d]">{revealErr}</div>
        )}

        <h4 className="mb-1.5 mt-4 text-[12.5px] font-semibold">工具归属（M2）</h4>
        {d.attributions.length === 0 ? (
          <div className="text-[11.5px] text-[#6b7075]">无工具归属证据（诚实状态）</div>
        ) : (
          <div className="space-y-1.5">
            {d.attributions.map((a, i) => (
              <div key={i} className="flex flex-wrap items-center gap-2 text-[11.5px]">
                <b>{a.tool_name || a.tool_key}</b>
                <span
                  className={
                    "rounded border px-1.5 text-[10px] " + (CONF_BADGE[a.confidence] ?? "border-[#35373c] text-[#9aa0a6]")
                  }
                >
                  {confLabel(a.confidence)}
                </span>
                <span className="text-[#6b7075]">来源行为：{sourceBehaviorLabel(a.source_behavior)}</span>
              </div>
            ))}
          </div>
        )}

        <h4 className="mb-1.5 mt-4 text-[12.5px] font-semibold">观察事件（M3-B）</h4>
        {d.events.length === 0 ? (
          <div className="text-[11.5px] text-[#6b7075]">无差分事件</div>
        ) : (
          <div className="space-y-1">
            {d.events.map((e) => (
              <div key={e.id} className="flex gap-2 text-[11.5px]">
                <span className="w-36 shrink-0 text-[#6b7075]">{fmtTime(e.ts)}</span>
                <span>{eventKindLabel(e.kind)}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export function Files() {
  // 路由参数：#/files?path=<pathId>（磁盘地图下钻入口）
  const [startPathId, setStartPathId] = useState<number | null>(() => {
    const m = location.hash.match(/[?&]path=(\d+)/);
    return m ? Number(m[1]) : null;
  });
  const [current, setCurrent] = useState<number | null>(null);
  const [crumbs, setCrumbs] = useState<Crumb[] | null>(null);
  const [children, setChildren] = useState<BrowseChild[] | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchHits, setSearchHits] = useState<SearchHit[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [detail, setDetail] = useState<BrowsePathDetail | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const navigateTo = useCallback((pathId: number) => {
    setErr(null);
    setSearchHits(null);
    setSearchQuery("");
    setCurrent(pathId);
    api
      .browseBreadcrumb(pathId)
      .then(setCrumbs)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
    api
      .browseChildren(pathId)
      .then(setChildren)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
  }, []);

  useEffect(() => {
    if (startPathId !== null) {
      navigateTo(startPathId);
      setStartPathId(null);
    }
  }, [startPathId, navigateTo]);

  const runSearch = () => {
    const q = searchQuery.trim();
    if (!q) {
      setSearchHits(null);
      return;
    }
    setSearching(true);
    setErr(null);
    api
      .searchPaths(q, 60)
      .then(setSearchHits)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)))
      .finally(() => setSearching(false));
  };

  return (
    <div>
      <PageHeader
        title="文件浏览"
        subtitle={
          <>  
          基于上次扫描快照（各卷最新代）的只读浏览。这里出现的每个路径都是数据库中的
          真实记录；不读取文件内容；本页绝不删除、移动、重命名任何文件。
          快照可能过期——以扫描时间为准。
        
          </>
        }
      />

      {/* 搜索 */}
      <div className="mb-3 flex flex-wrap gap-2">
        <input
          className="w-64 rounded border border-[#35373c] bg-[#1f2023] px-2.5 py-1.5 text-[12.5px] text-[#e8eaed] outline-none focus:border-[#4cc2ff]"
          placeholder="按文件名搜索（快照内，前缀匹配）"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && runSearch()}
        />
        <button
          className="rounded border border-[#4cc2ff] px-3 py-1.5 text-[12px] text-[#4cc2ff] disabled:opacity-40"
          disabled={searching}
          onClick={runSearch}
        >
          {searching ? "搜索中…" : "搜索"}
        </button>
        {searchHits !== null && (
          <button
            className="rounded border border-[#35373c] px-3 py-1.5 text-[12px] text-[#9aa0a6]"
            onClick={() => setSearchHits(null)}
          >
            清除搜索
          </button>
        )}
      </div>

      {err && (
        <div className="mb-4 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-4 py-3 text-[12.5px] text-[#f2c744]">
          {err}
        </div>
      )}

      {/* 面包屑 */}
      {crumbs !== null && searchHits === null && (
        <div className="mb-2 flex flex-wrap items-center gap-1 text-[12px]">
          {crumbs.map((c, i) => (
            <span key={c.path_id} className="flex items-center gap-1">
              {i > 0 && <span className="text-[#6b7075]">›</span>}
              <button
                className={
                  c.path_id === current ? "font-semibold text-[#4cc2ff]" : "text-[#9aa0a6] hover:text-[#e8eaed]"
                }
                onClick={() => navigateTo(c.path_id)}
              >
                {c.name}
              </button>
            </span>
          ))}
        </div>
      )}

      {/* 内容区 */}
      {searchHits !== null ? (
        <Card>
          <div className="mb-1 text-[11.5px] text-[#6b7075]">
            搜索结果 {searchHits.length} 条（各卷最新代快照；按大小排序）
          </div>
          {searchHits.length === 0 ? (
            <div className="py-6 text-center text-[12px] text-[#6b7075]">无匹配（快照内不存在该名称前缀）</div>
          ) : (
            searchHits.map((h) => (
              <div
                key={h.path_id}
                className="flex cursor-pointer flex-wrap items-center gap-2 border-b border-[#35373c] px-1 py-1.5 text-[12px] last:border-0 hover:bg-[#2c2e33]"
                onClick={() => (h.is_dir ? navigateTo(h.path_id) : openDetail(h.path_id))}
              >
                <span className={"break-all " + (h.is_dir ? "text-[#4cc2ff]" : "text-[#e8eaed]")}>
                  {h.abs_path}
                </span>
                <span className="flex-1" />
                <span className="text-[#6b7075]">{formatBytes(h.size)}</span>
              </div>
            ))
          )}
        </Card>
      ) : current === null ? (
        <Card>
          <div className="py-6 text-center">
            <div className="text-[13px] text-[#9aa0a6]">从磁盘地图进入，或直接搜索文件名</div>
            <div className="mt-1.5 text-[12px] text-[#6b7075]">
              磁盘地图页 → 选择一个已扫描的卷 → 「浏览该卷目录分布」
            </div>
          </div>
        </Card>
      ) : children === null ? (
        <div className="text-[12px] text-[#9aa0a6]">读取中…</div>
      ) : children.length === 0 ? (
        <Card>
          <div className="py-6 text-center text-[12px] text-[#6b7075]">
            该目录在快照中没有子项。注意：快照子项为空 ≠ 实际目录为空——
            可能是空目录，也可能该目录位于覆盖缺口内。
          </div>
        </Card>
      ) : (
        <Card>
          {children.map((c) => (
            <div
              key={c.path_id}
              className="flex cursor-pointer flex-wrap items-center gap-2 border-b border-[#35373c] px-1 py-1.5 text-[12px] last:border-0 hover:bg-[#2c2e33]"
              onClick={() => (c.is_dir ? navigateTo(c.path_id) : openDetail(c.path_id))}
            >
              <span className={c.is_dir ? "text-[#4cc2ff]" : "text-[#e8eaed]"}>
                {c.is_dir ? "▸" : "·"} {c.name}
              </span>
              <span className="flex-1" />
              <span className="text-[#6b7075]">{formatBytes(c.is_dir ? c.size : c.size)}</span>
              <span className="w-36 text-right text-[#6b7075]">{fmtTime(c.mtime)}</span>
            </div>
          ))}
        </Card>
      )}

      {detail !== null && <DetailDrawer d={detail} onClose={() => setDetail(null)} />}
    </div>
  );

  function openDetail(pathId: number) {
    setErr(null);
    api
      .browsePathDetail(pathId)
      .then(setDetail)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
  }
}
