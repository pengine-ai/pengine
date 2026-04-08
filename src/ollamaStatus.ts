import { OLLAMA_API_BASE } from "./config";

export type OllamaProbe = {
  reachable: boolean;
  model: string | null;
};

/** Prefer a loaded model from `/api/ps`, else first pulled model from `/api/tags`. */
export async function fetchOllamaModel(timeoutMs = 3000): Promise<OllamaProbe> {
  try {
    const psResp = await fetch(`${OLLAMA_API_BASE}/api/ps`, {
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (psResp.ok) {
      const psData = await psResp.json();
      const loaded = psData.models?.[0]?.name as string | undefined;
      if (loaded) {
        return { reachable: true, model: loaded };
      }
    }
    const tagsResp = await fetch(`${OLLAMA_API_BASE}/api/tags`, {
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (tagsResp.ok) {
      const tagsData = await tagsResp.json();
      const first = tagsData.models?.[0]?.name as string | undefined ?? null;
      return { reachable: true, model: first ?? null };
    }
    return { reachable: false, model: null };
  } catch {
    return { reachable: false, model: null };
  }
}
