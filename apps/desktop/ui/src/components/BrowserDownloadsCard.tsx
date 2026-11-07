/** 浏览器下载记录授权卡（M3-D · 默认关闭，逐浏览器×逐档案授权）。
 *  语义纪律：
 *  - 授权前不读任何浏览器数据（零行 = 功能完全关闭）；
 *  - 导入 = History 只读副本解析（副本即用即删），只取 downloads 最小字段集；
 *  - 下载记录 × 扫描路径 = 候选关联（不认定来源）；来源行为只能用户确认；
 *  - 撤销授权 = 删除该源全部导入记录；清除记录保留授权。
 */
import { useCallback, useEffect, useState } from "react";
import { Card } from "../components/EmptyState";
import {
  api,
  IpcError,
  type BrowserProfilesResponse,
  type BrowserImportStats,
  type DownloadRow,
} from "../lib/ipc";
import { formatBytes } from "../lib/format";

function fmtTime(ms: number | null): string {
  return ms ? new Date(ms).toLocaleString("zh-CN") : "—";
}

const STATE_BADGE: Record<string, string> = {
  complete: "border-[#6ccb5f]/40 text-[#6ccb5f]",
  in_progress: "border-[#4cc2ff]/40 text-[#4cc2ff]",
  cancelled: "border-[#35373c] text-[#6b7075]",
  interrupted: "border-[#f2c744]/40 text-[#f2c744]",
  unknown: "border-[#35373c] text-[#6b7075]",
};

const BROWSER_NAME: Record<string, string> = {
  chrome: "Google Chrome",
  edge: "Microsoft Edge",
};

export function BrowserDownloadsCard() {
  const [data, setData] = useState<BrowserProfilesResponse | null>(null);
  const [recent, setRecent] = useState<DownloadRow[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [revokeAsk, setRevokeAsk] = useState<number | null>(null);

  const load = useCallback(() => {
    setErr(null);
    api
      .browserListProfiles()
      .then(setData)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
    api
      .browserDownloadsList(null, 20)
      .then(setRecent)
      .catch(() => setRecent(null));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const authorize = async (browser: string, profile: string, historyPath: string) => {
    setBusy(true);
    setErr(null);
    setNote(null);
    try {
      await api.browserAuthorizeSource(browser, profile, historyPath);
      setNote(`已授权 ${BROWSER_NAME[browser] ?? browser} · ${profile}。点击「导入」读取下载记录。`);
      load();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const importNow = async (sourceId: number) => {
    setBusy(true);
    setErr(null);
    setNote(null);
    try {
      const s: BrowserImportStats = await api.browserImportSource(sourceId);
      if (s.error) {
        setErr(`导入失败：${s.error}`);
      } else {
        setNote(
          `导入完成：读取 ${s.fetched} 条（新增 ${s.inserted} / 更新 ${s.updated}），` +
            `候选匹配 ${s.matched_total} 条。匹配 = 候选关联，不代表来源。`
        );
      }
      load();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const toggleEnabled = async (sourceId: number, enabled: boolean) => {
    setBusy(true);
    try {
      await api.browserSetSourceEnabled(sourceId, enabled);
      load();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (sourceId: number) => {
    setBusy(true);
    try {
      const n = await api.browserRevokeSource(sourceId);
      setNote(`已撤销授权并删除 ${n} 条导入记录。`);
      setRevokeAsk(null);
      load();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const purge = async (sourceId: number) => {
    setBusy(true);
    try {
      const n = await api.browserPurgeSourceRecords(sourceId);
      setNote(`已清除 ${n} 条导入记录（授权保留）。`);
      load();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const authorizedKeys = new Set((data?.sources ?? []).map((s) => `${s.browser}|${s.profile}`));
  const fresh = (data?.profiles ?? []).filter((p) => !authorizedKeys.has(`${p.browser}|${p.profile}`));

  return (
    <Card title="浏览器下载记录（M3-D · 默认关闭，逐源授权）">
      <p className="mb-3 text-[11.5px] leading-relaxed text-[#9aa0a6]">
        授权后 FMap 对 History 文件做<b>只读复制</b>（不打开原库、不与浏览器进程交互），
        只解析 downloads 表的最小字段集；<b>绝不</b>读取浏览历史、Cookie 或密码。
        下载记录与扫描结果的匹配只是<b>候选关联</b>，不代表来源。
      </p>

      {err && (
        <div className="mb-3 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-3 py-2 text-[12px] text-[#f2c744]">
          {err}
        </div>
      )}
      {note && (
        <div className="mb-3 rounded-md border border-[#6ccb5f]/40 bg-[#6ccb5f]/10 px-3 py-2 text-[12px] text-[#6ccb5f]">
          {note}
        </div>
      )}

      {/* 已授权源 */}
      {data && data.sources.length > 0 && (
        <div className="mb-3 space-y-2">
          {data.sources.map((s) => (
            <div key={s.id} className="rounded-md border border-[#35373c] bg-[#1f2023] px-3 py-2 text-[12px]">
              <div className="flex flex-wrap items-center gap-2">
                <b>{BROWSER_NAME[s.browser] ?? s.browser}</b>
                <span className="text-[#9aa0a6]">{s.profile}</span>
                <span
                  className={
                    "rounded border px-1.5 text-[10.5px] " +
                    (s.enabled ? "border-[#6ccb5f]/40 text-[#6ccb5f]" : "border-[#f2c744]/40 text-[#f2c744]")
                  }
                >
                  {s.enabled ? "已启用" : "已暂停"}
                </span>
                <span className="text-[#9aa0a6]">
                  记录 {s.records} · 候选匹配 {s.matched}
                </span>
                {s.last_import_at && (
                  <span className="text-[#6b7075]">上次导入 {fmtTime(s.last_import_at)}</span>
                )}
              </div>
              <div className="mt-2 flex flex-wrap gap-2">
                <button
                  className="rounded border border-[#4cc2ff] px-2.5 py-1 text-[11px] text-[#4cc2ff] disabled:opacity-40"
                  disabled={busy || !s.enabled}
                  onClick={() => importNow(s.id)}
                >
                  导入
                </button>
                <button
                  className="rounded border border-[#35373c] px-2.5 py-1 text-[11px] text-[#9aa0a6] disabled:opacity-40"
                  disabled={busy}
                  onClick={() => toggleEnabled(s.id, !s.enabled)}
                >
                  {s.enabled ? "暂停" : "恢复"}
                </button>
                <button
                  className="rounded border border-[#35373c] px-2.5 py-1 text-[11px] text-[#9aa0a6] disabled:opacity-40"
                  disabled={busy || s.records === 0}
                  onClick={() => purge(s.id)}
                >
                  清除记录（保留授权）
                </button>
                <button
                  className="rounded border border-[#f26d6d]/50 px-2.5 py-1 text-[11px] text-[#f26d6d] disabled:opacity-40"
                  disabled={busy}
                  onClick={() => setRevokeAsk(s.id)}
                >
                  撤销授权
                </button>
              </div>
              {revokeAsk === s.id && (
                <div className="mt-2 rounded border border-[#f26d6d]/40 px-2.5 py-2 text-[11.5px] text-[#f26d6d]">
                  撤销将删除该源全部导入记录（{s.records} 条）。浏览器数据本身不受影响。
                  <div className="mt-1.5 flex gap-2">
                    <button
                      className="rounded border border-[#f26d6d]/60 px-2 py-0.5 text-[11px] text-[#f26d6d]"
                      disabled={busy}
                      onClick={() => revoke(s.id)}
                    >
                      确认撤销
                    </button>
                    <button
                      className="rounded border border-[#35373c] px-2 py-0.5 text-[11px] text-[#9aa0a6]"
                      onClick={() => setRevokeAsk(null)}
                    >
                      取消
                    </button>
                  </div>
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {/* 未授权的档案（发现列表） */}
      <div className="mb-3 text-[11.5px] text-[#9aa0a6]">本机发现的浏览器档案：</div>
      {data === null ? (
        <div className="text-[12px] text-[#9aa0a6]">读取中…</div>
      ) : fresh.length === 0 ? (
        <div className="mb-3 text-[12px] text-[#6b7075]">
          {data.profiles.length === 0
            ? "未发现 Chrome/Edge 档案（未安装或无 History 文件）。"
            : "全部已授权。"}
        </div>
      ) : (
        <div className="mb-3 space-y-1.5">
          {fresh.map((p) => (
            <div
              key={`${p.browser}|${p.profile}`}
              className="flex flex-wrap items-center gap-2 rounded-md border border-[#35373c] px-3 py-2 text-[12px]"
            >
              <b>{p.browser_name}</b>
              <span className="text-[#9aa0a6]">{p.profile}</span>
              <span className="flex-1" />
              <button
                className="rounded border border-[#4cc2ff] px-2.5 py-1 text-[11px] text-[#4cc2ff] disabled:opacity-40"
                disabled={busy}
                onClick={() => authorize(p.browser, p.profile, p.history_path)}
              >
                授权
              </button>
            </div>
          ))}
        </div>
      )}

      {/* 最近导入的下载记录 */}
      {recent !== null && recent.length > 0 && (
        <>
          <div className="mb-1.5 text-[11.5px] text-[#9aa0a6]">最近导入（最多 20 条）：</div>
          <div className="rounded-md border border-[#35373c]">
            {recent.map((d) => (
              <div
                key={d.id}
                className="flex flex-wrap items-center gap-2 border-b border-[#35373c] px-2.5 py-1.5 text-[11.5px] last:border-0"
              >
                <span
                  className={
                    "rounded border px-1.5 text-[10px] " +
                    (STATE_BADGE[d.state] ?? "border-[#35373c] text-[#6b7075]")
                  }
                >
                  {d.state}
                </span>
                <span className="break-all text-[#e8eaed]">{d.target_path}</span>
                <span className="text-[#6b7075]">{formatBytes(d.total_bytes)}</span>
                <span
                  className={
                    "rounded border px-1.5 text-[10px] " +
                    (d.match_state === "matched"
                      ? "border-[#6ccb5f]/40 text-[#6ccb5f]"
                      : "border-[#35373c] text-[#6b7075]")
                  }
                  title={d.match_state === "matched" ? "下载目标与扫描路径匹配（候选关联）" : "未匹配到扫描路径"}
                >
                  {d.match_state === "matched" ? "已匹配（候选）" : "未匹配"}
                </span>
                <span className="flex-1" />
                <span className="text-[#6b7075]">{fmtTime(d.end_time)}</span>
              </div>
            ))}
          </div>
        </>
      )}
    </Card>
  );
}
