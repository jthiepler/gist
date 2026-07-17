import type { ModelInfo, ModelsResult } from "./types";

export interface AvailableModel {
  name: string;
  title: string;
  caption: string;
  display: string;
  backend: string;
  sizeGb: number;
  description: string;
}

// Keep the catalog in the UI so the settings page does not depend on a
// sidecar round-trip just to render the available models.
export const AVAILABLE_LLM_MODELS: readonly AvailableModel[] = [
  {
    name: "qwen-3.5-4b",
    title: "Base",
    caption: "Qwen 3.5 · 4 billion parameters",
    display: "Qwen 3.5 4B",
    backend: "mlx",
    sizeGb: 2.5,
    description: "Fast everyday note generation",
  },
  {
    name: "qwen-3.5-9b",
    title: "Medium",
    caption: "Qwen 3.5 · 9 billion parameters",
    display: "Qwen 3.5 9B",
    backend: "mlx",
    sizeGb: 5.5,
    description: "More detailed notes, with slower processing",
  },
];

export const DEFAULT_LLM = "qwen-3.5-4b";
export const EVIDENCE_LLM = DEFAULT_LLM;

export function recommendedLlmForMemory(totalMemoryGb: number): string {
  return totalMemoryGb >= 16 ? "qwen-3.5-9b" : DEFAULT_LLM;
}

export function createModelState(downloaded: boolean | null = null): ModelsResult {
  return {
    llm: Object.fromEntries(
      AVAILABLE_LLM_MODELS.map((model): [string, ModelInfo] => [
        model.name,
        {
          display: model.display,
          backend: model.backend,
          size_gb: model.sizeGb,
          description: model.description,
          downloaded,
        },
      ]),
    ),
  };
}

export function mergeDownloadedState(result: ModelsResult): ModelsResult {
  const models = createModelState();
  for (const [name, info] of Object.entries(models.llm)) {
    info.downloaded = result.llm[name]?.downloaded ?? null;
  }
  return models;
}
