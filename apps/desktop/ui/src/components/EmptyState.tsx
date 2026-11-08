/** 通用组件：开发中空状态 + 通用卡片。 */
import type { ReactNode } from "react";

export function Card(props: { title?: string; children: ReactNode; className?: string }) {
  return (
    <div
      className={
        "rounded-md border border-[#35373c] bg-[#24262a] p-4 transition-colors hover:border-[#45484f] " +
        (props.className ?? "")
      }
    >
      {props.title && <h3 className="mb-3 text-[13px] font-semibold">{props.title}</h3>}
      {props.children}
    </div>
  );
}

/** 明确的开发状态占位：说明由哪个里程碑实现，不展示任何假数据。 */
export function DevPlaceholder(props: { feature: string; milestone: string; note?: string }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 rounded-md border border-dashed border-[#35373c] bg-[#1f2023] px-6 py-12 text-center">
      <div className="text-[15px] font-semibold text-[#9aa0a6]">{props.feature}</div>
      <div className="rounded border border-[#f2c744]/40 px-2 py-0.5 text-[11px] text-[#f2c744]">
        计划于 {props.milestone} 实现 · 当前尚未开发
      </div>
      {props.note && <div className="max-w-md text-[12px] leading-relaxed text-[#6b7075]">{props.note}</div>}
    </div>
  );
}
