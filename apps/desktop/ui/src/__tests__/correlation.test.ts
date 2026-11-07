/** M3-C 关联展示纯函数测试（vitest 真实执行）。 */
import { describe, expect, it } from "vitest";
import {
  corrConfidenceLabel,
  corrSignalKindLabel,
  fmtDelta,
  PAGE_IDS,
  reviewStateLabel,
} from "../lib/format";

describe("M3-C 标签", () => {
  it("审查状态四词表完整，未知值原样返回", () => {
    expect(reviewStateLabel("pending")).toBe("待审查");
    expect(reviewStateLabel("confirmed")).toBe("已确认关联");
    expect(reviewStateLabel("denied")).toBe("已否认");
    expect(reviewStateLabel("ignored")).toBe("已忽略");
    expect(reviewStateLabel("bogus")).toBe("bogus");
  });

  it("置信标签不出现'已验证'语义（时间关联永远不是确证）", () => {
    expect(corrConfidenceLabel("likely")).toContain("特征文件");
    expect(corrConfidenceLabel("possible")).toContain("数据目录");
    for (const label of [corrConfidenceLabel("likely"), corrConfidenceLabel("possible")]) {
      expect(label).not.toContain("已验证");
      expect(label).not.toContain("确证");
    }
  });

  it("信号种类标签", () => {
    expect(corrSignalKindLabel("signal_path")).toBe("特征文件");
    expect(corrSignalKindLabel("data_dir")).toBe("工具数据目录");
    expect(corrSignalKindLabel("other")).toBe("other");
  });
});

describe("fmtDelta", () => {
  it("各量级正确", () => {
    expect(fmtDelta(30 * 1000)).toBe("30 秒");
    expect(fmtDelta(20 * 60 * 1000)).toBe("20 分钟");
    expect(fmtDelta(2.5 * 60 * 60 * 1000)).toBe("2.5 小时");
    expect(fmtDelta(30 * 24 * 60 * 60 * 1000)).toBe("30.0 天");
  });
  it("异常输入返回占位符", () => {
    expect(fmtDelta(null)).toBe("—");
    expect(fmtDelta(-1)).toBe("—");
    expect(fmtDelta(Number.NaN)).toBe("—");
  });
});

describe("路由", () => {
  it("新增 correlation 页面（10 页）", () => {
    expect(PAGE_IDS).toHaveLength(10);
    expect(PAGE_IDS).toContain("correlation");
  });
});
