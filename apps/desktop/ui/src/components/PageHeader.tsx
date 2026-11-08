/** 统一页头：标题 + 副标题 + 右侧动作槽（界面一致性打磨）。 */
import type { ReactNode } from "react";

export function PageHeader(props: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-4 flex flex-wrap items-end justify-between gap-3">
      <div className="min-w-0">
        <h1 className="text-[19px] font-semibold">{props.title}</h1>
        {props.subtitle && (
          <div className="mt-1 max-w-4xl text-[12px] leading-relaxed text-[#9aa0a6]">
            {props.subtitle}
          </div>
        )}
      </div>
      {props.actions && <div className="flex shrink-0 items-center gap-2">{props.actions}</div>}
    </div>
  );
}

/** 统计条（Overview/报告页的真实数据摘要）。 */
export function StatStrip(props: {
  items: { label: string; value: number | string; accent?: string; title?: string }[];
}) {
  return (
    <div className="mb-4 grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-5">
      {props.items.map((s) => (
        <div
          key={s.label}
          title={s.title}
          className="rounded-md border border-[#35373c] bg-[#24262a] px-3 py-2.5 transition-colors hover:border-[#4a4d55]"
        >
          <div className="text-[11px] text-[#6b7075]">{s.label}</div>
          <div className={"text-[17px] font-semibold " + (s.accent ?? "text-[#e8eaed]")}>
            {s.value}
          </div>
        </div>
      ))}
    </div>
  );
}

/** 空状态 + 行动按钮（引导用户到能产生数据的页面，而非干巴巴一句话）。 */
export function EmptyWithAction(props: {
  title: string;
  hint: string;
  actionLabel?: string;
  actionHash?: string;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-2.5 rounded-md border border-dashed border-[#35373c] bg-[#1f2023] px-6 py-12 text-center">
      <div className="text-[14px] text-[#9aa0a6]">{props.title}</div>
      <div className="max-w-md text-[12px] leading-relaxed text-[#6b7075]">{props.hint}</div>
      {props.actionLabel && props.actionHash && (
        <a
          href={props.actionHash}
          className="mt-1.5 rounded border border-[#4cc2ff] px-3 py-1 text-[12px] text-[#4cc2ff] transition-colors hover:bg-[#4cc2ff]/10"
        >
          {props.actionLabel}
        </a>
      )}
    </div>
  );
}
