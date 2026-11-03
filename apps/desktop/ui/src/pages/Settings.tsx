/** 设置页（M0）：只读展示默认策略与隐私承诺；设置持久化在 M1 实现。 */
import { Card } from "../components/EmptyState";

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

export function Settings() {
  return (
    <div>
      <h1 className="text-[19px] font-semibold">设置</h1>
      <p className="mb-5 mt-1 text-[12px] text-[#9aa0a6]">
        M0 阶段展示默认策略（已确定的最终决策）；设置持久化 UI 与读写将在 M1 交付。
      </p>
      <div className="grid grid-cols-1 gap-3.5 xl:grid-cols-2">
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
          <ToggleRow title="读取文件正文 / 内容级 AI 分析" desc="默认关闭。M0–M1 阶段完全不存在此代码路径。" state="默认关闭" tone="off" />
          <ToggleRow title="计算文件内容哈希" desc="默认关闭（仅路径元数据）。" state="默认关闭" tone="off" />
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
