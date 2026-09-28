/**
 * A model as a person reads its name: the repo's last path segment, as it is spelled
 * (`Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx` → `Qwen3.8-27B-Atlassian-Q8-mlx`). The ONE rule
 * every sentence that names a model uses (Q-308: one model had four spellings on one page); the
 * repo id itself belongs in Details. goose-sidecar's `model_identity::model_short_name` is the same
 * rule, and both are pinned to `modelShortName.fixture.json`.
 */
export function modelShortName(modelId: string): string {
  return modelId.split('/').filter(Boolean).pop() ?? modelId;
}
