import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import { deleteConnect } from "../api";

type AppSessionState = {
  isDeviceConnected: boolean;
  botUsername: string | null;
  botId: string | null;
  connectDevice: (bot?: { bot_username: string; bot_id?: string | null }) => void;
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
          botId: bot?.bot_id != null && bot.bot_id !== "" ? bot.bot_id : null,
        }),

      disconnectDevice: async () => {
        await deleteConnect();
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
