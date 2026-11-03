/**
 * M0 阶段的占位页面：磁盘地图 / 文件浏览 / AI 工具 / 项目资产 / 历史追踪 /
 * 实时监控 / 扫描报告。每个页面只说明"将实现什么、何时实现、依赖什么"，
 * 不渲染任何演示数据（产品原则 3：真实数据优先于模拟数据）。
 */
import { DevPlaceholder } from "../components/EmptyState";

export function DiskMap() {
  return (
    <Page title="磁盘地图" sub="Treemap 可视化：面积 = 占用，颜色 = AI 归属">
      <DevPlaceholder
        feature="Treemap 磁盘地图"
        milestone="M1"
        note="依赖扫描引擎交付真实数据后接入。渲染策略：可见节点上限 + 下钻（见 docs/architecture/SYSTEM_DESIGN.md §4）。"
      />
    </Page>
  );
}

export function Files() {
  return (
    <Page title="文件浏览" sub="只读浏览扫描结果，附来源详情抽屉（PRD FR-9）">
      <DevPlaceholder
        feature="文件浏览器"
        milestone="M1"
        note="M0 阶段不提供任何文件列表——扫描引擎尚未实现，不使用模拟数据。"
      />
    </Page>
  );
}

export function Tools() {
  return (
    <Page title="AI 工具" sub="AI 软件识别中心：检测状态、占用、置信度">
      <DevPlaceholder
        feature="AI 工具识别"
        milestone="M2"
        note="依赖 AI 工具目录规则库 v1（docs/research/AI_TOOLS_DIRECTORIES.md 已完成路径调研）。"
      />
    </Page>
  );
}

export function Projects() {
  return (
    <Page title="项目资产" sub="以项目为单元聚合资产，V0.2 引入跨盘指纹">
      <DevPlaceholder feature="项目资产" milestone="M2" />
    </Page>
  );
}

export function History() {
  return (
    <Page title="历史追踪" sub="快照差分 + 事件时间线，全部标注置信度">
      <DevPlaceholder feature="历史时间线" milestone="V0.2" note="历史重建基于快照差分（诚实版），不做 USN 全盘回放（需管理员权限，已排除）。" />
    </Page>
  );
}

export function Monitor() {
  return (
    <Page title="实时监控" sub="notify 实时事件流（Windows: ReadDirectoryChangesW）">
      <DevPlaceholder feature="实时监控" milestone="M5" note="事件不携带进程归属——ETW 需管理员权限，已被产品决策排除（docs/research/TECH_FEASIBILITY.md Q4）。" />
    </Page>
  );
}

export function Reports() {
  return (
    <Page title="扫描报告" sub="工具占用 / 卸载安全清单 / 空间变化报告">
      <DevPlaceholder feature="报告中心" milestone="V0.2" note="导出由系统另存为对话框完成，FMap 不擅自写入任何位置。" />
    </Page>
  );
}

function Page(props: { title: string; sub: string; children: React.ReactNode }) {
  return (
    <div>
      <h1 className="text-[19px] font-semibold">{props.title}</h1>
      <p className="mb-5 mt-1 text-[12px] text-[#9aa0a6]">{props.sub}</p>
      {props.children}
    </div>
  );
}
