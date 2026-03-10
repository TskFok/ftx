import { describe, it, expect } from "vitest";
import { isConnectionError } from "./connectionError";

describe("connectionError", () => {
  describe("isConnectionError", () => {
    it("识别空闲超时错误", () => {
      expect(
        isConnectionError(
          "Connection closed due to idle timeout (300 seconds)",
        ),
      ).toBe(true);
    });

    it("识别无活跃连接错误", () => {
      expect(
        isConnectionError("No active connection for host 1"),
      ).toBe(true);
    });

    it("识别 Connection closed 错误", () => {
      expect(isConnectionError("Connection closed")).toBe(true);
    });

    it("非连接错误返回 false", () => {
      expect(isConnectionError("Permission denied")).toBe(false);
      expect(isConnectionError("File not found")).toBe(false);
      expect(isConnectionError(new Error("Network error"))).toBe(false);
    });
  });
});
