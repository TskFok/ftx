import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { FileEntry } from "../types";
import { isConnectionError } from "../utils/connectionError";

interface FileBrowserState {
  localPath: string;
  remotePath: string;
  localFiles: FileEntry[];
  remoteFiles: FileEntry[];
  localLoading: boolean;
  remoteLoading: boolean;
  selectedLocalFiles: string[];
  selectedRemoteFiles: string[];
  connectedHostId: number | null;

  setLocalPath: (path: string) => void;
  setRemotePath: (path: string) => void;
  setConnectedHostId: (id: number | null) => void;
  clearConnectionState: () => void;
  fetchLocalFiles: (path?: string) => Promise<void>;
  fetchRemoteFiles: (hostId: number, path?: string) => Promise<void>;
  setRemoteFiles: (files: FileEntry[]) => void;
  setSelectedLocalFiles: (paths: string[]) => void;
  setSelectedRemoteFiles: (paths: string[]) => void;
  refreshLocal: () => Promise<void>;
  refreshRemote: () => Promise<void>;
}

export const useFileBrowserStore = create<FileBrowserState>((set, get) => ({
  localPath: "",
  remotePath: "/",
  localFiles: [],
  remoteFiles: [],
  localLoading: false,
  remoteLoading: false,
  selectedLocalFiles: [],
  selectedRemoteFiles: [],
  connectedHostId: null,

  setLocalPath: (path) => set({ localPath: path }),
  setRemotePath: (path) => set({ remotePath: path }),
  setConnectedHostId: (id) => set({ connectedHostId: id }),
  clearConnectionState: () =>
    set({
      connectedHostId: null,
      remoteFiles: [],
      remotePath: "/",
      selectedRemoteFiles: [],
    }),

  fetchLocalFiles: async (path?: string) => {
    const targetPath = path ?? get().localPath;
    set({ localLoading: true });
    try {
      const files = await invoke<FileEntry[]>("list_local_dir", {
        path: targetPath,
      });
      set({ localFiles: files, localPath: targetPath, selectedLocalFiles: [] });
    } finally {
      set({ localLoading: false });
    }
  },

  fetchRemoteFiles: async (hostId: number, path?: string) => {
    const targetPath = path ?? get().remotePath;
    set({ remoteLoading: true });
    try {
      const files = await invoke<FileEntry[]>("list_remote_dir", {
        hostId,
        path: targetPath,
      });
      set({
        remoteFiles: files,
        remotePath: targetPath,
        connectedHostId: hostId,
        selectedRemoteFiles: [],
      });
    } catch (err) {
      if (isConnectionError(err)) {
        get().clearConnectionState();
      }
      throw err;
    } finally {
      set({ remoteLoading: false });
    }
  },

  setRemoteFiles: (files) => set({ remoteFiles: files }),
  setSelectedLocalFiles: (paths) => set({ selectedLocalFiles: paths }),
  setSelectedRemoteFiles: (paths) => set({ selectedRemoteFiles: paths }),

  refreshLocal: async () => {
    const { localPath } = get();
    if (localPath) {
      await get().fetchLocalFiles(localPath);
    }
  },

  refreshRemote: async () => {
    const { connectedHostId, remotePath } = get();
    if (connectedHostId) {
      await get().fetchRemoteFiles(connectedHostId, remotePath);
    }
  },
}));
