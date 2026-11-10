/** 设置页：默认策略与隐私承诺（只读展示）+ 文件身份采集授权卡（M3-A，交互式）。
 *  身份采集是 M3-A 唯一的隐私增量：新增「卷+File ID」元数据采集，
 *  不读正文、不算哈希、不上传——默认关闭，可暂停、可撤销（撤销时是否
 *  删除已采数据由用户明确选择）。
 */
import { useCallback, useEffect, useState } from "react";
import { Card } from "../components/EmptyState";
import { PageHeader } from "../components/PageHeader";
import { BrowserDownloadsCard } from "../components/BrowserDownloadsCard";
import { api, IpcError, type IdentitySettings, type RevokeStats } from "../lib/ipc";
import { sourceBehaviorLabel } from "../lib/format";

function ToggleRow(props: { title: string; desc: string; state: string; tone: "on" | "off" | "plan" }) {
  const toneClass =
    props.tone === "on" ? "text-[#6ccb5f]" : props.tone === "off" ? "text-[#f2c744]" : "text-[#6b7075]";
  return (
    <div className="flex items-center justify-between gap-6 border-b border-[#35373c] py-3 last:border-0">
      <div>
        <div className="text-[13px]">{props.title}</div>
        <div className="mt-0.5 max-w-xl text-[11.5px] leading-relaxed text-[#6b7075]">{props.desc}</div>
      </div>
      <span className={"shrink-0 text-[12px] font-semibold " + toneClass}>{props.state}</span>
    </div>
  );
}

function IdentityCollectionCard() {
  const [settings, setSettings] = useState<IdentitySettings | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [revokeAsk, setRevokeAsk] = useState(false);
  const [revokeResult, setRevokeResult] = useState<RevokeStats | null>(null);

  const load = useCallback(() => {
    api
      .getIdentitySettings()
      .then(setSettings)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const toggle = async () => {
    if (!settings) return;
    setBusy(true);
    setErr(null);
    try {
      await api.setIdentityCollection(!settings.enabled);
      load();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (deleteData: boolean) => {
    setBusy(true);
    setErr(null);
    try {
      const stats = await api.revokeIdentityCollection(deleteData);
      setRevokeResult(stats);
      setRevokeAsk(false);
      load();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const o = settings?.overview;
  return (
    <Card title="文件身份采集（M3-A · 默认关闭，逐项授权）">
      {!settings && !err && <div className="text-[12.5px] text-[#9aa0a6]">读取中…</div>}
      {err && (
        <div className="mb-2 rounded-md border border-[#f2c744]/40 bg-[#f2c744]/10 px-3 py-2 text-[12px] text-[#f2c744]">
          {err}
        </div>
      )}
      {settings && (
        <>
          <ToggleRow
            title="授权采集文件身份（卷 GUID + NTFS FileId）"
            desc="仅新增元数据：不读取文件正文、不计算内容哈希、不上传任何数据。授权后可在总览页对已扫描磁盘运行身份采集；暂停（关闭开关）不删除任何已采集数据。"
            state={settings.enabled ? "已授权" : "默认关闭"}
            tone={settings.enabled ? "on" : "off"}
          />
          <div className="mt-3 flex flex-wrap items-center gap-2.5">
            <button
              className="rounded bg-[#4cc2ff] px-3.5 py-1.5 text-[12.5px] font-semibold text-white disabled:opacity-40"
              disabled={busy}
              onClick={toggle}
            >
              {settings.enabled ? "暂停采集" : "授权并开启"}
            </button>
            <button
              className="rounded border border-[#f26d6d] px-3 py-1.5 text-[12.5px] text-[#f26d6d] disabled:opacity-40"
              disabled={busy || o?.entities === 0}
              onClick={() => setRevokeAsk(true)}
              title={o?.entities === 0 ? "尚无身份数据" : "停止并可选删除已采数据"}
            >
              撤销授权…
            </button>
          </div>
          {o && (
            <div className="mt-3 grid grid-cols-2 gap-x-6 gap-y-1 text-[12px] text-[#9aa0a6] sm:grid-cols-3">
              <span>文件实体：{o.entities}</span>
              <span>主身份（FileId）：{o.with_file_id}</span>
              <span>弱身份（降级）：{o.weak_identity}</span>
              <span>已确认来源：{o.confirmed_entities}</span>
              <span>自动候选：{o.candidates}</span>
              <span>事件：{o.events}</span>
            </div>
          )}
          {revokeAsk && (
            <div className="mt-3 rounded-md border border-[#f26d6d]/40 bg-[#f26d6d]/5 p-3 text-[12px]">
              <div className="mb-2 font-semibold text-[#f26d6d]">撤销授权后是否删除已采集的身份数据？</div>
              <div className="mb-3 text-[11.5px] text-[#9aa0a6]">
                两种选择都会立即停止身份采集。删除操作只影响身份数据（文件实体/事件/证据/候选/确认），
                不会触碰你的任何文件，也不影响已扫描的路径与归属记录。
              </div>
              <div className="flex gap-2.5">
                <button
                  className="rounded border border-[#35373c] px-3 py-1.5 text-[12px]"
                  disabled={busy}
                  onClick={() => revoke(false)}
                >
                  仅停止（保留数据）
                </button>
                <button
                  className="rounded bg-[#f26d6d] px-3 py-1.5 text-[12px] font-semibold text-white"
                  disabled={busy}
                  onClick={() => revoke(true)}
                >
                  停止并删除身份数据
                </button>
                <button className="px-2 text-[12px] text-[#9aa0a6] underline" disabled={busy} onClick={() => setRevokeAsk(false)}>
                  取消
                </button>
              </div>
            </div>
          )}
          {revokeResult && (
            <div className="mt-3 rounded-md border border-[#6ccb5f]/40 bg-[#6ccb5f]/5 px-3 py-2 text-[12px] text-[#6ccb5f]">
              {revokeResult.deleted_data
                ? `已撤销并删除身份数据（实体 ${revokeResult.entities} / 事件 ${revokeResult.events} / 证据 ${revokeResult.evidence} / 候选 ${revokeResult.candidates} / 确认 ${revokeResult.confirmations}）`
                : "已撤销授权（数据保留，可随时重新授权）"}
              <button className="ml-2 text-[11px] text-[#9aa0a6] underline" onClick={() => setRevokeResult(null)}>
                知道了
              </button>
            </div>
          )}
          <div className="mt-3 text-[11px] leading-relaxed text-[#6b7075]">
            语义承诺：文件身份 ≠ 文件来源。来源行为（AI 创建/修改等）只能由你手动确认，
            自动分析永远显示「未知」；未确认时绝不编造来源。
            {o && o.confirmed_entities > 0 && (
              <>
                {" "}当前已有 {o.confirmed_entities} 个文件经你确认（
                {o.confirmed_entities > 0 ? "来源行为见 AI 工具页详情" : ""}，标签如「{sourceBehaviorLabel("ai_created")}」）。
              </>
            )}
          </div>
        </>
      )}
    </Card>
  );
}

/** 差分事件上限卡（M5）：查看/提高全局上限——超限处置（历史页）的配套。
 *  上限默认 50,000/对；提高后 pending_over_limit 的差分对在补跑时自动重试。
 */
function DiffLimitCard() {
  const [limit, setLimit] = useState<number | null>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => {
    api
      .getDiffLimit()
      .then(setLimit)
      .catch((e: unknown) => setErr(e instanceof IpcError ? e.message : String(e)));
  }, []);
  useEffect(() => {
    load();
  }, [load]);
  const raise = async () => {
    const n = Number(input);
    if (!Number.isFinite(n) || n <= 0) {
      setErr("请输入正整数");
      return;
    }
    setBusy(true);
    setErr(null);
    setNote(null);
    try {
      const eff = await api.diffRaiseLimit(n);
      setNote(`已生效：${eff.toLocaleString()} 事件/对。被阻塞的差分对在「历史追踪 → 补跑差分」时自动重试。`);
      load();
    } catch (e: unknown) {
      setErr(e instanceof IpcError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Card title="差分事件上限（超限处置配套）">
      <p className="mb-3 text-[11.5px] leading-relaxed text-[#9aa0a6]">
        相邻两次扫描的差分一次产出超过该上限时，差分会被标记为「已阻塞（待人工处理）」
        而不是写半截数据。处置入口在历史追踪页的被阻塞行上；这里查看并调整全局上限。
      </p>
      <div className="mb-2 text-[13px]">
        当前生效上限：<b className="text-[#e8eaed]">{limit === null ? "—" : limit.toLocaleString()}</b> 事件/对
        {limit !== null && limit !== 50000 && (
          <span className="ml-2 text-[10.5px] text-[#f2c744]">（已人工调整，默认 50,000）</span>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <input
          className="w-32 rounded border border-[#35373c] bg-[#1f2023] px-2 py-1 font-mono text-[12px] text-[#e8eaed] outline-none focus:border-[#4cc2ff]"
          placeholder="新上限（如 100000）"
          value={input}
          onChange={(e) => setInput(e.target.value.replace(/[^\d]/g, ""))}
        />
        <button
          className="rounded border border-[#4cc2ff] px-2.5 py-1 text-[11.5px] text-[#4cc2ff] disabled:opacity-40"
          disabled={busy || !input}
          onClick={raise}
        >
          提高上限
        </button>
        <span className="text-[11px] text-[#6b7075]">允许范围 1,000 ~ 5,000,000</span>
      </div>
      {note && <div className="mt-2 text-[11.5px] text-[#6ccb5f]">{note}</div>}
      {err && <div className="mt-2 text-[11.5px] text-[#f26d6d]">{err}</div>}
    </Card>
  );
}

export function Settings() {
  return (
    <div>
      <PageHeader
        title="设置"
        subtitle={
          <>  
          默认策略（已确定的最终决策）+ 隐私敏感功能的逐项授权（默认关闭）。
        
          </>
        }
      />
      <div className="grid grid-cols-1 gap-3.5 xl:grid-cols-2">
        <IdentityCollectionCard />
        <BrowserDownloadsCard />
        <DiffLimitCard />
        <Card title="扫描范围（最终决策：所有磁盘）">
          <ToggleRow
            title="默认扫描所有磁盘"
            desc="自动发现可访问的已挂载磁盘并纳入扫描范围；权限不足的目录记录错误并继续，不中断扫描。"
            state="默认开启"
            tone="on"
          />
          <ToggleRow
            title="不读取需要提权的区域"
            desc="不使用 MFT 直读等需要管理员权限的机制（技术边界见 docs/research/TECH_FEASIBILITY.md）。"
            state="硬性约束"
            tone="on"
          />
        </Card>
        <Card title="隐私敏感功能（最终决策：默认关闭，逐项授权）">
          <ToggleRow title="读取 AI 会话数据库 / 聊天记录 / 会话 JSONL" desc="默认关闭。首次开启前将逐项说明读取内容、原因、保存位置。" state="默认关闭" tone="off" />
          <ToggleRow title="读取文件正文 / 内容级 AI 分析" desc="默认关闭。M0–M3 阶段完全不存在此代码路径。" state="默认关闭" tone="off" />
          <ToggleRow title="计算文件内容哈希" desc="默认关闭（仅路径与身份元数据）。" state="默认关闭" tone="off" />
        </Card>
        <Card title="只读承诺">
          <ToggleRow title="不删除 / 修改 / 移动 / 重命名任何用户文件" desc="产品永不提供清理功能；CI 中有只读门禁（tests/check-readonly.mjs）。" state="硬性约束" tone="on" />
          <ToggleRow title="不修改注册表 / 环境变量 / 不加入 Defender 排除" desc="永久承诺。" state="硬性约束" tone="on" />
        </Card>
        <Card title="网络">
          <ToggleRow title="零网络出站" desc="不上传任何文件路径、名称、内容或报告；不连接第三方 AI API；无账号、无 API Key。" state="硬性约束" tone="on" />
        </Card>
      </div>
    </div>
  );
}
