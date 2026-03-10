import { describe, it, expect, vi, beforeEach } from "vitest";
import { useSettingsStore } from "./settingsStore";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  vi.clearAllMocks();
  useSettingsStore.setState({
    idleTimeoutSecs: 300,
    loading: false,
  });
});

describe("settingsStore", () => {
  describe("fetchIdleTimeout", () => {
    it("获取空闲超时并更新状态", async () => {
      mockInvoke.mockResolvedValueOnce(600);

      await useSettingsStore.getState().fetchIdleTimeout();

      expect(mockInvoke).toHaveBeenCalledWith("get_idle_timeout_secs");
      expect(useSettingsStore.getState().idleTimeoutSecs).toBe(600);
      expect(useSettingsStore.getState().loading).toBe(false);
    });

    it("获取失败时使用默认值", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("db error"));

      await useSettingsStore.getState().fetchIdleTimeout();

      expect(useSettingsStore.getState().idleTimeoutSecs).toBe(300);
    });
  });

  describe("setIdleTimeout", () => {
    it("设置空闲超时并更新状态", async () => {
      mockInvoke.mockResolvedValueOnce(undefined);

      await useSettingsStore.getState().setIdleTimeout(600);

      expect(mockInvoke).toHaveBeenCalledWith("set_idle_timeout_secs", {
        secs: 600,
      });
      expect(useSettingsStore.getState().idleTimeoutSecs).toBe(600);
    });

    it("设置失败时抛出错误", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("invalid value"));

      await expect(
        useSettingsStore.getState().setIdleTimeout(99999)
      ).rejects.toThrow("invalid value");
    });
  });
});
