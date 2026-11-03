/** IPC 封装层测试：核心是"浏览器模式绝不伪造数据"。 */
import { afterEach, describe, expect, it, vi } from "vitest";

// backendAvailable 在模块加载时确定，因此用 vi.resetModules + 动态导入
// 来模拟两种运行环境（浏览器直开 / Tauri 内）。
// 注意：@tauri-apps/api 的 invoke 通过 window.__TAURI_INTERNALS__.invoke 工作，
// mock 必须挂在该路径上。

afterEach(() => {
  vi.resetModules();
  delete (globalThis as Record<string, unknown>).window;
});

describe("ipc 封装", () => {
  it("浏览器模式：backendAvailable=false，调用命令抛 IpcError(kind=backend)，不返回假数据", async () => {
    delete (globalThis as Record<string, unknown>).window;
    const { api, IpcError, backendAvailable } = await import("../lib/ipc");
    expect(backendAvailable).toBe(false);
    const err = await api.getSystemStatus().then(
      () => null,
      (e) => e
    );
    expect(err).toBeInstanceOf(IpcError);
    expect((err as InstanceType<typeof IpcError>).kind).toBe("backend");
    expect((err as InstanceType<typeof IpcError>).message).toContain("后端未连接");
  });

  it("Tauri 模式：透传 invoke 结果", async () => {
    (globalThis as Record<string, unknown>).window = {
      __TAURI_INTERNALS__: {
        invoke: (cmd: string) => {
          if (cmd === "get_app_info") {
            return Promise.resolve({ name: "FMap", version: "0.1.0", platform: "windows", elevated: false });
          }
          return Promise.reject(new Error("unknown command: " + cmd));
        },
      },
    };
    const { api, backendAvailable } = await import("../lib/ipc");
    expect(backendAvailable).toBe(true);
    const info = await api.getAppInfo();
    expect(info.version).toBe("0.1.0");
    expect(info.elevated).toBe(false);
  });

  it("Tauri 模式：后端结构化错误 {kind,message} 还原为 IpcError", async () => {
    (globalThis as Record<string, unknown>).window = {
      __TAURI_INTERNALS__: {
        invoke: () => Promise.reject({ kind: "platform", message: "macOS / Linux 支持尚未实现" }),
      },
    };
    const { api, IpcError } = await import("../lib/ipc");
    const err = await api.listVolumes().then(
      () => null,
      (e) => e
    );
    expect(err).toBeInstanceOf(IpcError);
    expect((err as InstanceType<typeof IpcError>).kind).toBe("platform");
    expect((err as InstanceType<typeof IpcError>).message).toContain("尚未实现");
  });
});
