import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export const DEFAULT_IDLE_TIMEOUT_SECS = 300;

export interface SettingsState {
  idleTimeoutSecs: number;
  loading: boolean;
  fetchIdleTimeout: () => Promise<void>;
  setIdleTimeout: (secs: number) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  idleTimeoutSecs: DEFAULT_IDLE_TIMEOUT_SECS,
  loading: false,

  fetchIdleTimeout: async () => {
    set({ loading: true });
    try {
      const secs = await invoke<number>("get_idle_timeout_secs");
      set({ idleTimeoutSecs: secs });
    } catch {
      set({ idleTimeoutSecs: DEFAULT_IDLE_TIMEOUT_SECS });
    } finally {
      set({ loading: false });
    }
  },

  setIdleTimeout: async (secs: number) => {
    try {
      await invoke("set_idle_timeout_secs", { secs });
      set({ idleTimeoutSecs: secs });
    } catch (e) {
      throw e;
    }
  },
}));
