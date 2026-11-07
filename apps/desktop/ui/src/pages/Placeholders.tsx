/**
 * 剩余占位页面：项目资产 / 扫描报告。每个页面只说明"将实现什么、何时实现、
 * 依赖什么"，不渲染任何演示数据（产品原则 3：真实数据优先于模拟数据）。
 * 磁盘地图 / 文件浏览 / 实时监控已在 M3-E/M4 落地为真实页面。
 */
import { DevPlaceholder } from "../components/EmptyState";

export function Projects() {
  return (
    <DevPlaceholder
      feature="项目资产"
      milestone="M2"
      note="以项目为单元聚合资产，V0.2 引入跨盘指纹。"
    />
  );
}

export function Reports() {
  return (
    <DevPlaceholder
      feature="报告中心"
      milestone="V0.2"
      note="导出由系统另存为对话框完成，FMap 不擅自写入任何位置。"
    />
  );
}
