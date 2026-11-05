/** 前端纯函数测试（vitest 真实执行）。 */
import { describe, expect, it } from "vitest";
import {
  diffRunStatusLabel,
  eventKindLabel,
  eventNote,
  formatBytes,
  isPageId,
  pageFromHash,
  PAGE_IDS,
  sourceActionLabel,
  sourceBehaviorLabel,
} from "../lib/format";

describe("formatBytes", () => {
  it("各量级正确", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
    expect(formatBytes(3 * 1024 ** 3)).toBe("3.0 GB");
    expect(formatBytes(2 * 1024 ** 4)).toBe("2.0 TB");
  });
  it("异常输入返回占位符而非崩溃", () => {
    expect(formatBytes(null)).toBe("—");
    expect(formatBytes(-5)).toBe("—");
    expect(formatBytes(Number.NaN)).toBe("—");
  });
});

describe("hash 路由", () => {
  it("9 个页面 id 完整", () => {
    expect(PAGE_IDS).toHaveLength(9);
  });
  it("合法与非法 hash", () => {
    expect(pageFromHash("#/overview")).toBe("overview");
    expect(pageFromHash("#/settings")).toBe("settings");
    expect(pageFromHash("#/nonexistent")).toBe("overview");
    expect(pageFromHash("")).toBe("overview");
    expect(isPageId("tools")).toBe(true);
    expect(isPageId("tools2")).toBe(false);
  });
});

describe("M3-A 来源语义标签", () => {
  it("来源行为映射完整且未知值原样返回", () => {
    expect(sourceBehaviorLabel("ai_created")).toBe("AI 创建");
    expect(sourceBehaviorLabel("ai_modified")).toBe("AI 修改");
    expect(sourceBehaviorLabel("ai_downloaded")).toBe("AI 下载");
    expect(sourceBehaviorLabel("user_created")).toBe("用户创建");
    expect(sourceBehaviorLabel("user_modified")).toBe("用户修改");
    expect(sourceBehaviorLabel("unknown")).toBe("未知");
    expect(sourceBehaviorLabel("ai_maybe")).toBe("ai_maybe");
  });
  it("确认动作与事件类型映射", () => {
    expect(sourceActionLabel("confirm")).toBe("确认");
    expect(sourceActionLabel("correct")).toBe("修正");
    expect(sourceActionLabel("revoke")).toBe("撤销");
    expect(eventKindLabel("source_confirmed")).toBe("来源确认");
    expect(eventKindLabel("source_corrected")).toBe("来源修正");
    expect(eventKindLabel("file_id_reuse_suspected")).toBe("身份重用嫌疑（待确认）");
    expect(eventKindLabel("file_discovered")).toBe("首次观察到"); // M3-B1 起有语义标签
  });
});

describe("M3-B1 差分事件与台账标签", () => {
  it("四类差分事件文案遵守语义红线", () => {
    expect(eventKindLabel("file_discovered")).toBe("首次观察到");
    expect(eventKindLabel("file_modified")).toBe("观察到变化");
    expect(eventKindLabel("file_missing")).toBe("缺失（待确认）");
    expect(eventKindLabel("file_reappeared")).toBe("重新出现");
    // 红线：文案不得出现"创建""删除"等断言
    for (const k of ["file_discovered", "file_modified", "file_missing", "file_reappeared"]) {
      const label = eventKindLabel(k);
      expect(label).not.toContain("创建");
      expect(label).not.toContain("删除");
    }
  });
  it("台账状态与 note 提取", () => {
    expect(diffRunStatusLabel("completed")).toBe("已完成研判");
    expect(diffRunStatusLabel("pending_over_limit")).toBe("已阻塞（事件量超限，待人工处理）");
    expect(diffRunStatusLabel("weird")).toBe("weird");
    expect(eventNote(null)).toBe("");
    expect(eventNote("{\"note\":\"abc\"}")).toBe("abc");
    expect(eventNote("not-json")).toBe("");
  });
});
