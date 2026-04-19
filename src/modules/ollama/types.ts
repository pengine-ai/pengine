export type OllamaProbe = { reachable: boolean; model: string | null };

export type OllamaModelKind = "local" | "cloud";

export type OllamaModelInfo = {
  name: string;
  kind: OllamaModelKind;
};

export type OllamaModelsResponse = {
  reachable: boolean;
  active_model: string | null;
  selected_model: string | null;
  models: OllamaModelInfo[];
};
