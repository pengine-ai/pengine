/**
 * Heuristics for splitting MCP stdio `env` into a primary secret (dedicated UI) vs other KEY=value lines.
 */

export function isLikelySecretEnvKey(key: string): boolean {
  const k = key.trim();
  if (!k) return false;
  if (/api[_-]?key|secret|token|password|auth|bearer|credential|private/i.test(k)) return true;
  if (/_KEY$/i.test(k)) return true;
  return false;
}

/** Prefer a clearly sensitive key; otherwise first `*_API_KEY`-style name. */
export function extractPrimarySecretEnvKey(env: Record<string, string>): string | null {
  const keys = Object.keys(env);
  const hit = keys.find(isLikelySecretEnvKey);
  if (hit) return hit;
  return keys.find((k) => /_API_KEY$/i.test(k)) ?? null;
}

export function envToOtherLinesText(
  env: Record<string, string>,
  excludeKey: string | null,
): string {
  const skip = excludeKey?.trim() ?? "";
  return Object.entries(env)
    .filter(([k]) => (skip ? k !== skip : true))
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
}

export type BuildEnvMapOptions = {
  otherLinesText: string;
  apiKeyName: string;
  apiKeyValue: string;
  /** When the UI is masked and the user is not replacing, copy this from the saved entry. */
  preservedSecretValue: string | null;
  replacingSecret: boolean;
};

/**
 * Merge textarea lines with the dedicated API key row. Empty name → no dedicated row.
 * When `replacingSecret` and value empty → omit key (clear secret).
 */
export function buildEnvMapFromMcpForm(opts: BuildEnvMapOptions): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of opts.otherLinesText.split("\n")) {
    const t = line.trim();
    if (!t) continue;
    const eq = t.indexOf("=");
    if (eq > 0) {
      const k = t.slice(0, eq).trim();
      if (k) out[k] = t.slice(eq + 1).trim();
    }
  }
  const name = opts.apiKeyName.trim();
  if (!name) return out;

  delete out[name];

  if (opts.replacingSecret) {
    const v = opts.apiKeyValue.trim();
    if (v) out[name] = v;
    return out;
  }

  const preserved = opts.preservedSecretValue?.trim() ?? "";
  if (preserved) {
    out[name] = preserved;
    return out;
  }
  const v = opts.apiKeyValue.trim();
  if (v) out[name] = v;
  return out;
}
