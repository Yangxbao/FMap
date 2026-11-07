/** 实时监控（M3-E/M3-B2）：授权根运行状态 + 差分缺口 + 原始通知 + USN 能力画像。
 *  语义纪律：原始通知 = 观察层（零去重含系统重复）；USN 能力 = 真实探测
 *  （卷级需管理员→产品红线内不启用并如实展示降级，单文件 USN 已实现）。
 */
import { useCallback, useEffect, useState } from "react";
import { Card } from "../components/EmptyState";
import {
  api,
  IpcError,
  type MonitorPageData,
  type WatchRootStatus,
  type WatchNotifyRow,
  type WatchGap,
} from "../lib/ipc";

function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

export function Monitor() {
  const [mon, setMon] = useState<MonitorPageData | null>(null);
  const [roots, setRoots] = useState<WatchRootStatus[] | null>(null);
  const [gaps, setGaps] = useState<WatchGap[] | null>(null);
  const [notifies, setNotifies] = useState<WatchNotifyRow[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => {
    setErr(null);
    api
      .getMonitorData()
      .then(setMon)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
    api
      .watchList()
      .then(setRoots)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
    api
      .watchGapList()
      .then(setGaps)
      .catch(() => setGaps(null));
    api
      .watchRecentNotifications(30)
      .then(setNotifies)
      .catch(() => setNotifies(null));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div>
      <h1 className="text-[19px] font-semibold">实时监控</h1>
      <p className="mb-2 mt-1 text-[12px] leading-relaxed text-[#9aa0a6]">
        逐目录授权的 RDCW 采集（设置页授权）+ 变更日志能力画像。
        事件不携带进程归属——ETW 需管理员权限，已被产品决策排除。
        卷级 USN Journal 同样需要管理员，在产品红线内<b>不启用</b>，能力探测如实展示。
      </p>

      {err && (
        <div className="mb-4 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-4 py-3 text-[12.5px] text-[#f2c744]">
          {err}
        </div>
      )}

      {/* 监控授权根状态 */}
      <h2 className="mb-2 mt-4 text-[14px] font-semibold">监控授权根状态</h2>
      {roots === null ? (
        <div className="text-[12px] text-[#9aa0a6]">读取中…</div>
      ) : roots.length === 0 ? (
        <div className="rounded-md border border-dashed border-[#35373c] bg-[#1f2023] px-6 py-6 text-center text-[12px] text-[#6b7075]">
          尚无监控授权（设置页 → 目录实时监控卡，逐目录授权并启动）。
        </div>
      ) : (
        <Card>
          {roots.map((r) => (
            <div
              key={r.path}
              className="flex flex-wrap items-center gap-2 border-b border-[#35373c] px-1 py-2 text-[12px] last:border-0"
            >
              <span
                className={
                  "rounded border px-1.5 text-[10.5px] " +
                  (r.watching
                    ? "border-[#6ccb5f]/40 text-[#6ccb5f]"
                    : r.enabled
                      ? "border-[#f2c744]/40 text-[#f2c744]"
                      : "border-[#35373c] text-[#6b7075]")
                }
              >
                {r.watching ? "采集中" : r.enabled ? "已授权未启动" : "已暂停"}
              </span>
              <b className="break-all">{r.path}</b>
              <span className="text-[#9aa0a6]">接收 {r.recv_seq}</span>
              <span className="text-[#9aa0a6]">持久化 {r.persisted_seq}</span>
              {r.overflow_gen > 0 && (
                <span className="rounded border border-[#f2c744]/40 px-1.5 text-[10.5px] text-[#f2c744]">
                  溢出代 {r.overflow_gen}
                </span>
              )}
              {r.has_open_gap && (
                <span className="rounded border border-[#f26d6d]/50 px-1.5 text-[10.5px] text-[#f26d6d]">
                  对账缺口未关闭
                </span>
              )}
            </div>
          ))}
        </Card>
      )}

      {/* 监控缺口 */}
      {gaps !== null && gaps.length > 0 && (
        <>
          <h2 className="mb-2 mt-4 text-[14px] font-semibold">监控缺口（溢出对账）</h2>
          <Card>
            {gaps.map((g) => (
              <div key={g.id} className="border-b border-[#35373c] px-1 py-2 text-[12px] last:border-0">
                <b className="break-all">{g.watch_root}</b>
                <span
                  className={
                    "ml-2 rounded border px-1.5 text-[10.5px] " +
                    (g.status === "opened"
                      ? "border-[#f26d6d]/50 text-[#f26d6d]"
                      : "border-[#6ccb5f]/40 text-[#6ccb5f]")
                  }
                >
                  {g.status === "opened" ? "未对账" : "已对账"}
                </span>
                <span className="ml-2 text-[#6b7075]">
                  开于 {fmtTime(g.opened_at)}
                  {g.closed_at ? ` · 关于 ${fmtTime(g.closed_at)}` : ""}
                </span>
              </div>
            ))}
          </Card>
        </>
      )}

      {/* 原始通知（观察层） */}
      {notifies !== null && notifies.length > 0 && (
        <>
          <h2 className="mb-2 mt-4 text-[14px] font-semibold">最近原始通知（观察层，含系统重复）</h2>
          <Card>
            {notifies.map((n) => (
              <div key={n.id} className="flex gap-2 border-b border-[#35373c] px-1 py-1.5 text-[11.5px] last:border-0">
                <span className="w-36 shrink-0 text-[#6b7075]">{fmtTime(n.ts)}</span>
                <span className="text-[#4cc2ff]">{n.kind}</span>
                <span className="break-all text-[#9aa0a6]">{n.path}</span>
              </div>
            ))}
          </Card>
        </>
      )}

      {/* USN 能力画像 */}
      <h2 className="mb-2 mt-4 text-[14px] font-semibold">变更日志能力画像（USN Journal · 真实探测）</h2>
      {mon === null ? (
        <div className="text-[12px] text-[#9aa0a6]">探测中…</div>
      ) : (
        <Card>
          {mon.journal_capabilities.map((c) => (
            <div key={c.volume} className="border-b border-[#35373c] px-1 py-2.5 text-[12px] last:border-0">
              <div className="flex flex-wrap items-center gap-2">
                <b>{c.volume}</b>
                <span className="rounded border border-[#35373c] px-1.5 text-[10.5px] text-[#9aa0a6]">
                  {c.fs_type ?? "未知文件系统"}
                </span>
                {c.usn_supported_fs ? (
                  <span className="rounded border border-[#6ccb5f]/40 px-1.5 text-[10.5px] text-[#6ccb5f]">
                    文件系统支持 USN
                  </span>
                ) : (
                  <span className="rounded border border-[#f2c744]/40 px-1.5 text-[10.5px] text-[#f2c744]">
                    该文件系统无 USN Journal
                  </span>
                )}
              </div>
              <div className="mt-1.5 space-y-1 text-[11.5px] leading-relaxed text-[#9aa0a6]">
                <div>
                  <b className="text-[#e8eaed]">卷级 USN：</b>
                  不可用 — {c.volume_usn.denial_reason ?? "未知原因"}
                </div>
                <div>
                  <b className="text-[#e8eaed]">单文件 USN（FSCTL_READ_FILE_USN_DATA）：</b>
                  {c.file_usn_available ? (
                    <span className="text-[#6ccb5f]">可用（无需管理员，已实现）</span>
                  ) : (
                    <span className="text-[#f26d6d]">不可用（{c.file_usn_error ?? "未知"}）</span>
                  )}
                </div>
              </div>
            </div>
          ))}
          <div className="mt-2 text-[11px] leading-relaxed text-[#6b7075]">
            降级说明：USN 不可用不影响事件完整性——扫描差分（M3-B1）与 RDCW 监控（M3-B2）
            独立工作。详见 docs/research/USN_JOURNAL_M3E_DESIGN.md。
          </div>
        </Card>
      )}
    </div>
  );
}
