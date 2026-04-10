import { OLLAMA_API_BASE } from "../../../shared/api/config";
import { PENGINE_API_BASE } from "../../../shared/api/config";
import type { OllamaModelsResponse, OllamaProbe } from "../types";

/** Prefer loaded model from `/api/ps`, else first pulled model from `/api/tags`. */
export async function fetchOllamaModel(timeoutMs = 3000): Promise<OllamaProbe> {
  try {
    const psResp = await fetch(`${OLLAMA_API_BASE}/api/ps`, {
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (psResp.ok) {
      const psData = await psResp.json();
      const loaded = psData.models?.[0]?.name as string | undefined;
      if (loaded) return { reachable: true, model: loaded };
    }
    const tagsResp = await fetch(`${OLLAMA_API_BASE}/api/tags`, {
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (tagsResp.ok) {
      const tagsData = await tagsResp.json();
      const first = (tagsData.models?.[0]?.name as string | undefined) ?? null;
      return { reachable: true, model: first ?? null };
    }
    return { reachable: false, model: null };
  } catch {
    return { reachable: false, model: null };
  }
}

export async function fetchOllamaModels(timeoutMs = 3000): Promise<OllamaModelsResponse> {
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/ollama/models`, {
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (!resp.ok) {
      return { reachable: false, active_model: null, selected_model: null, models: [] };
    }
    return (await resp.json()) as OllamaModelsResponse;
  } catch {
    return { reachable: false, active_model: null, selected_model: null, models: [] };
  }
}

export async function setPreferredOllamaModel(model: string | null): Promise<boolean> {
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/ollama/model`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model }),
      signal: AbortSignal.timeout(5000),
    });
    return resp.ok;
  } catch {
    return false;
  }
}
