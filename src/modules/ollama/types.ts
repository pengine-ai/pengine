export type OllamaProbe = { reachable: boolean; model: string | null };

export type OllamaModelsResponse = {
  reachable: boolean;
  active_model: string | null;
  selected_model: string | null;
  models: string[];
};
