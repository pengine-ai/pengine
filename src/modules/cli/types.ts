/** Tauri `cli_shim_*` (writes `pengine-cli` launcher) — serde uses camelCase. */
export type CliShimStatus = {
  shimPath: string;
  installed: boolean;
  resolvesTo: string | null;
  localBinOnPath: boolean;
  pathExportHint: string;
};
