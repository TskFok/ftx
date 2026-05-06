import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ConfigProvider } from "antd";
import zhCN from "antd/locale/zh_CN";
import type { FileEntry } from "../../types";
import TransferActionBar from "./TransferActionBar";
import { invoke } from "@tauri-apps/api/core";

const mockStartUpload = vi.fn(() => Promise.resolve("transfer-upload-1"));
const mockStartDownload = vi.fn(() => Promise.resolve("transfer-download-1"));
const mockStartDirectoryUpload = vi.fn(() => Promise.resolve([] as string[]));
const mockStartDirectoryDownload = vi.fn(() => Promise.resolve([] as string[]));
const mockShowDialog = vi.fn();
const mockResetOverwriteAll = vi.fn();
const mockClearConnectionState = vi.fn();

const hoisted = vi.hoisted(() => {
  const fileBrowserPartial: {
    localFiles: FileEntry[];
    selectedLocalFiles: string[];
    selectedRemoteFiles: string[];
    remoteFiles: FileEntry[];
    connectedHostId: number | null;
    remotePath: string;
    localPath: string;
  } = {
    localFiles: [],
    selectedLocalFiles: [],
    selectedRemoteFiles: [],
    remoteFiles: [],
    connectedHostId: null,
    remotePath: "/",
    localPath: "",
  };
  return { fileBrowserPartial };
});

const { fileBrowserPartial } = hoisted;

const localSample: FileEntry = {
  name: "a.txt",
  path: "/local/a.txt",
  is_dir: false,
  size: 100,
  modified: undefined,
};

vi.mock("../../stores/transferStore", () => ({
  useTransferStore: (sel: (s: unknown) => unknown) =>
    sel({
      startUpload: mockStartUpload,
      startDownload: mockStartDownload,
      startDirectoryUpload: mockStartDirectoryUpload,
      startDirectoryDownload: mockStartDirectoryDownload,
    }),
}));

vi.mock("../../stores/fileBrowserStore", () => ({
  useFileBrowserStore: (sel?: (s: unknown) => unknown) => {
    const state = {
      ...fileBrowserPartial,
      clearConnectionState: mockClearConnectionState,
    };
    return typeof sel === "function" ? sel(state) : state;
  },
}));

vi.mock("../../stores/overwriteStore", () => ({
  useOverwriteStore: (sel: (s: unknown) => unknown) =>
    sel({
      showDialog: mockShowDialog,
      resetOverwriteAll: mockResetOverwriteAll,
    }),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function renderBar() {
  return render(
    <ConfigProvider locale={zhCN}>
      <TransferActionBar />
    </ConfigProvider>,
  );
}

describe("TransferActionBar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.assign(fileBrowserPartial, {
      localFiles: [localSample],
      selectedLocalFiles: [localSample.path],
      selectedRemoteFiles: [],
      remoteFiles: [],
      connectedHostId: 1,
      remotePath: "/remote",
      localPath: "/local",
    });
    mockShowDialog.mockResolvedValue("overwrite");
  });

  it("远端已存在且选择覆盖时只调用一次 startUpload", async () => {
    fileBrowserPartial.remoteFiles = [
      { name: "a.txt", path: "/remote/a.txt", is_dir: false, size: 1, modified: undefined },
    ];
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "remote_file_exists") return Promise.resolve(true);
      return Promise.resolve(null);
    });

    const user = userEvent.setup();
    renderBar();
    const buttons = screen.getAllByRole("button");
    await user.click(buttons[0]);

    expect(mockShowDialog).toHaveBeenCalledTimes(1);
    expect(mockStartUpload).toHaveBeenCalledTimes(1);
  });

  it("远端不存在时不弹窗且只上传一次", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "remote_file_exists") return Promise.resolve(false);
      return Promise.resolve(null);
    });

    const user = userEvent.setup();
    renderBar();
    const buttons = screen.getAllByRole("button");
    await user.click(buttons[0]);

    expect(mockShowDialog).not.toHaveBeenCalled();
    expect(mockStartUpload).toHaveBeenCalledTimes(1);
  });

  it("本地已存在且选择覆盖时只调用一次 startDownload", async () => {
    const remoteSample: FileEntry = {
      name: "a.txt",
      path: "/remote/a.txt",
      is_dir: false,
      size: 100,
      modified: undefined,
    };
    fileBrowserPartial.selectedLocalFiles = [];
    fileBrowserPartial.remoteFiles = [remoteSample];
    fileBrowserPartial.selectedRemoteFiles = [remoteSample.path];

    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "check_local_file_exists") return Promise.resolve(true);
      return Promise.resolve(null);
    });

    const user = userEvent.setup();
    renderBar();
    const buttons = screen.getAllByRole("button");
    await user.click(buttons[1]);

    expect(mockShowDialog).toHaveBeenCalledTimes(1);
    expect(mockStartDownload).toHaveBeenCalledTimes(1);
  });
});
