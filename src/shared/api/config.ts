/** Local Pengine connection API (Tauri `connection_server`, loopback only). */
export const PENGINE_API_BASE = "http://127.0.0.1:21516";

/** Default Ollama HTTP API (same host as typical desktop install). */
export const OLLAMA_API_BASE = "http://localhost:11434";

/** Browsers often report timeouts as AbortError / “Fetch is aborted”. */
export function fetchErrorMessage(e: unknown): string {
  if (e instanceof DOMException && e.name === "AbortError") {
    return "Request timed out — the app may still be working (e.g. reconnecting MCP or pulling an image). Wait and refresh, or check the in-app log.";
  }
  if (e instanceof Error) {
    const m = e.message.toLowerCase();
    if (m.includes("abort")) {
      return "Request timed out — the app may still be working (e.g. reconnecting MCP or pulling an image). Wait and refresh, or check the in-app log.";
    }
    return e.message;
  }
  return "Request failed";
}
