import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

const PENGINE_API = "http://127.0.0.1:21516";

type AppSessionState = {
  isDeviceConnected: boolean;
  botUsername: string | null;
  botId: string | null;
  connectDevice: (bot?: { bot_username: string; bot_id: string }) => void;
  disconnectDevice: () => Promise<void>;
};

export const useAppSessionStore = create<AppSessionState>()(
  persist(
    (set) => ({
      isDeviceConnected: false,
      botUsername: null,
      botId: null,

      connectDevice: (bot) =>
        set({
          isDeviceConnected: true,
          botUsername: bot?.bot_username ?? null,
          botId: bot?.bot_id ?? null,
        }),

      disconnectDevice: async () => {
        try {
          await fetch(`${PENGINE_API}/v1/connect`, {
            method: "DELETE",
            signal: AbortSignal.timeout(5000),
          });
        } catch {
          // local app may be unreachable; clear session anyway
        }
        set({ isDeviceConnected: false, botUsername: null, botId: null });
      },
    }),
    {
      name: "pengine-device-session",
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        isDeviceConnected: state.isDeviceConnected,
        botUsername: state.botUsername,
        botId: state.botId,
      }),
    },
  ),
);
