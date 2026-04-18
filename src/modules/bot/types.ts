export type PengineHealth = {
  status: string;
  bot_connected: boolean;
  bot_username?: string;
  bot_id?: string | null;
  app_version?: string;
  git_commit?: string;
};
