import { invoke } from "@tauri-apps/api/core";
import type { CliShimStatus } from "../types";

export async function cliShimStatus(): Promise<CliShimStatus | null> {
  try {
    return await invoke<CliShimStatus>("cli_shim_status");
  } catch {
    return null;
  }
}

export async function cliShimInstall(): Promise<
  { ok: true; status: CliShimStatus } | { ok: false; error: string }
> {
  try {
    const status = await invoke<CliShimStatus>("cli_shim_install");
    return { ok: true, status };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return { ok: false, error: msg };
  }
}

export async function cliShimRemove(): Promise<{ ok: true } | { ok: false; error: string }> {
  try {
    await invoke("cli_shim_remove");
    return { ok: true };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return { ok: false, error: msg };
  }
}
