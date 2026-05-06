import { describe, it, expect, vi, beforeEach } from "vitest";
import { useFileBrowserStore } from "./fileBrowserStore";
import type { FileEntry } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

const sampleFiles: FileEntry[] = [
  { name: "docs", path: "/home/docs", is_dir: true, size: 0 },
  { name: "file.txt", path: "/home/file.txt", is_dir: false, size: 1024 },
];

const sampleRemoteFiles: FileEntry[] = [
  { name: "www", path: "/var/www", is_dir: true, size: 0 },
  { name: "index.html", path: "/var/index.html", is_dir: false, size: 512 },
];

beforeEach(() => {
  vi.clearAllMocks();
  useFileBrowserStore.setState({
    localPath: "",
    remotePath: "/",
    localFiles: [],
    remoteFiles: [],
    localLoading: false,
    remoteLoading: false,
    selectedLocalFiles: [],
    selectedRemoteFiles: [],
    connectedHostId: null,
  });
});

describe("fileBrowserStore", () => {
  describe("fetchLocalFiles", () => {
    it("获取本地文件列表", async () => {
      mockInvoke.mockResolvedValueOnce(sampleFiles);

      await useFileBrowserStore.getState().fetchLocalFiles("/home");

      expect(mockInvoke).toHaveBeenCalledWith("list_local_dir", {
        path: "/home",
      });
      const state = useFileBrowserStore.getState();
      expect(state.localFiles).toEqual(sampleFiles);
      expect(state.localPath).toBe("/home");
      expect(state.localLoading).toBe(false);
      expect(state.selectedLocalFiles).toEqual([]);
    });

    it("使用当前路径当不传参数时", async () => {
      useFileBrowserStore.setState({ localPath: "/tmp" });
      mockInvoke.mockResolvedValueOnce([]);

      await useFileBrowserStore.getState().fetchLocalFiles();

      expect(mockInvoke).toHaveBeenCalledWith("list_local_dir", {
        path: "/tmp",
      });
    });

    it("加载失败后 loading 恢复", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("Permission denied"));

      await expect(
        useFileBrowserStore.getState().fetchLocalFiles("/root"),
      ).rejects.toThrow();
      expect(useFileBrowserStore.getState().localLoading).toBe(false);
    });

    it("较晚返回的旧 list_local 结果不覆盖当前目录", async () => {
      let resolveSlow!: (v: FileEntry[]) => void;
      const slowPromise = new Promise<FileEntry[]>((r) => {
        resolveSlow = r;
      });
      const fastFiles: FileEntry[] = [
        { name: "n.txt", path: "/proj/n.txt", is_dir: false, size: 1 },
      ];

      mockInvoke.mockImplementation((cmd, args) => {
        if (
          cmd === "list_local_dir" &&
          args &&
          typeof args === "object" &&
          "path" in args &&
          args.path === "/slow_home"
        ) {
          return slowPromise;
        }
        if (cmd === "list_local_dir") {
          return Promise.resolve(fastFiles);
        }
        return Promise.resolve([]);
      });

      const pSlow = useFileBrowserStore.getState().fetchLocalFiles("/slow_home");
      await useFileBrowserStore.getState().fetchLocalFiles("/proj");

      resolveSlow([
        {
          name: "stale.txt",
          path: "/slow_home/stale.txt",
          is_dir: false,
          size: 2,
        },
      ]);
      await pSlow;

      const state = useFileBrowserStore.getState();
      expect(state.localPath).toBe("/proj");
      expect(state.localFiles).toEqual(fastFiles);
      expect(state.localLoading).toBe(false);
    });

    it("已被更新的 list_local 请求失败时不抛出", async () => {
      let rejectSlow!: (e: Error) => void;
      const slowPromise = new Promise<FileEntry[]>((_, rej) => {
        rejectSlow = rej;
      });
      mockInvoke.mockImplementation((cmd, args) => {
        if (
          cmd === "list_local_dir" &&
          args &&
          typeof args === "object" &&
          "path" in args &&
          args.path === "/slow_home"
        ) {
          return slowPromise;
        }
        return Promise.resolve([]);
      });

      const pSlow = useFileBrowserStore.getState().fetchLocalFiles("/slow_home");
      await useFileBrowserStore.getState().fetchLocalFiles("/ok");

      rejectSlow(new Error("stale should be ignored"));
      await expect(pSlow).resolves.toBeUndefined();
      expect(useFileBrowserStore.getState().localPath).toBe("/ok");
    });
  });

  describe("fetchRemoteFiles", () => {
    it("获取远程文件列表", async () => {
      mockInvoke.mockResolvedValueOnce(sampleRemoteFiles);

      await useFileBrowserStore.getState().fetchRemoteFiles(1, "/var");

      expect(mockInvoke).toHaveBeenCalledWith("list_remote_dir", {
        hostId: 1,
        path: "/var",
      });
      const state = useFileBrowserStore.getState();
      expect(state.remoteFiles).toEqual(sampleRemoteFiles);
      expect(state.remotePath).toBe("/var");
      expect(state.connectedHostId).toBe(1);
      expect(state.remoteLoading).toBe(false);
    });

    it("使用当前远程路径当不传 path 参数时", async () => {
      useFileBrowserStore.setState({ remotePath: "/home" });
      mockInvoke.mockResolvedValueOnce([]);

      await useFileBrowserStore.getState().fetchRemoteFiles(2);

      expect(mockInvoke).toHaveBeenCalledWith("list_remote_dir", {
        hostId: 2,
        path: "/home",
      });
    });

    it("较晚返回的旧 list_remote 结果不覆盖当前远程目录", async () => {
      let resolveSlow!: (v: FileEntry[]) => void;
      const slowPromise = new Promise<FileEntry[]>((r) => {
        resolveSlow = r;
      });
      const fastFiles: FileEntry[] = [
        { name: "b.txt", path: "/srv/b.txt", is_dir: false, size: 3 },
      ];

      mockInvoke.mockImplementation((cmd, args) => {
        if (
          cmd === "list_remote_dir" &&
          args &&
          typeof args === "object" &&
          "path" in args &&
          args.path === "/slow_remote"
        ) {
          return slowPromise;
        }
        if (cmd === "list_remote_dir") {
          return Promise.resolve(fastFiles);
        }
        return Promise.resolve([]);
      });

      const pSlow = useFileBrowserStore
        .getState()
        .fetchRemoteFiles(1, "/slow_remote");
      await useFileBrowserStore.getState().fetchRemoteFiles(1, "/srv");

      resolveSlow(sampleRemoteFiles);
      await pSlow;

      const state = useFileBrowserStore.getState();
      expect(state.remotePath).toBe("/srv");
      expect(state.remoteFiles).toEqual(fastFiles);
      expect(state.connectedHostId).toBe(1);
    });

    it("已被覆盖的 list_remote 若失败为连接错误也不清除连接状态", async () => {
      let rejectSlow!: (e: Error) => void;
      const slowPromise = new Promise<FileEntry[]>((_, rej) => {
        rejectSlow = rej;
      });
      mockInvoke.mockImplementation((cmd, args) => {
        if (
          cmd === "list_remote_dir" &&
          args &&
          typeof args === "object" &&
          "path" in args &&
          args.path === "/slow_remote"
        ) {
          return slowPromise;
        }
        return Promise.resolve([]);
      });

      useFileBrowserStore.setState({ connectedHostId: 1 });
      const pSlow = useFileBrowserStore
        .getState()
        .fetchRemoteFiles(1, "/slow_remote");
      await useFileBrowserStore.getState().fetchRemoteFiles(1, "/ok");

      rejectSlow(
        new Error("Connection closed due to idle timeout (300 seconds)"),
      );
      await pSlow;

      expect(useFileBrowserStore.getState().connectedHostId).toBe(1);
      expect(useFileBrowserStore.getState().remotePath).toBe("/ok");
    });

    it("连接断开错误时清除连接状态", async () => {
      useFileBrowserStore.setState({
        connectedHostId: 1,
        remoteFiles: sampleRemoteFiles,
        remotePath: "/var",
      });
      mockInvoke.mockRejectedValueOnce(
        new Error("Connection closed due to idle timeout (300 seconds)"),
      );

      await expect(
        useFileBrowserStore.getState().fetchRemoteFiles(1, "/var"),
      ).rejects.toThrow();

      const state = useFileBrowserStore.getState();
      expect(state.connectedHostId).toBeNull();
      expect(state.remoteFiles).toEqual([]);
      expect(state.remotePath).toBe("/");
      expect(state.selectedRemoteFiles).toEqual([]);
    });
  });

  describe("clearConnectionState", () => {
    it("清除连接相关状态", () => {
      useFileBrowserStore.setState({
        connectedHostId: 1,
        remoteFiles: sampleRemoteFiles,
        remotePath: "/var",
        selectedRemoteFiles: ["/var/a.txt"],
      });

      useFileBrowserStore.getState().clearConnectionState();

      const state = useFileBrowserStore.getState();
      expect(state.connectedHostId).toBeNull();
      expect(state.remoteFiles).toEqual([]);
      expect(state.remotePath).toBe("/");
      expect(state.selectedRemoteFiles).toEqual([]);
    });
  });

  describe("selection", () => {
    it("设置本地选中文件", () => {
      useFileBrowserStore
        .getState()
        .setSelectedLocalFiles(["/a.txt", "/b.txt"]);
      expect(useFileBrowserStore.getState().selectedLocalFiles).toEqual([
        "/a.txt",
        "/b.txt",
      ]);
    });

    it("设置远程选中文件", () => {
      useFileBrowserStore.getState().setSelectedRemoteFiles(["/r/c.txt"]);
      expect(useFileBrowserStore.getState().selectedRemoteFiles).toEqual([
        "/r/c.txt",
      ]);
    });
  });

  describe("connectedHostId", () => {
    it("设置已连接主机", () => {
      useFileBrowserStore.getState().setConnectedHostId(5);
      expect(useFileBrowserStore.getState().connectedHostId).toBe(5);
    });

    it("清除已连接主机", () => {
      useFileBrowserStore.setState({ connectedHostId: 3 });
      useFileBrowserStore.getState().setConnectedHostId(null);
      expect(useFileBrowserStore.getState().connectedHostId).toBeNull();
    });
  });

  describe("refreshLocal", () => {
    it("有路径时刷新", async () => {
      useFileBrowserStore.setState({ localPath: "/home" });
      mockInvoke.mockResolvedValueOnce(sampleFiles);

      await useFileBrowserStore.getState().refreshLocal();

      expect(mockInvoke).toHaveBeenCalledWith("list_local_dir", {
        path: "/home",
      });
    });

    it("无路径时不请求", async () => {
      useFileBrowserStore.setState({ localPath: "" });
      await useFileBrowserStore.getState().refreshLocal();
      expect(mockInvoke).not.toHaveBeenCalled();
    });
  });

  describe("refreshRemote", () => {
    it("已连接时刷新", async () => {
      useFileBrowserStore.setState({
        connectedHostId: 1,
        remotePath: "/var",
      });
      mockInvoke.mockResolvedValueOnce(sampleRemoteFiles);

      await useFileBrowserStore.getState().refreshRemote();

      expect(mockInvoke).toHaveBeenCalledWith("list_remote_dir", {
        hostId: 1,
        path: "/var",
      });
    });

    it("未连接时不请求", async () => {
      useFileBrowserStore.setState({ connectedHostId: null });
      await useFileBrowserStore.getState().refreshRemote();
      expect(mockInvoke).not.toHaveBeenCalled();
    });
  });
});
