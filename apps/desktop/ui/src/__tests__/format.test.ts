/** 前端纯函数测试（vitest 真实执行）。 */
import { describe, expect, it } from "vitest";
import { formatBytes, isPageId, pageFromHash, PAGE_IDS } from "../lib/format";

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
