import {
  CODEX_MODEL_OBSERVATION_SOURCE,
  type ProviderModelObservation,
} from "../provider-model-observation-store";
import { providerSupportsModel, resolveModelAlias } from "./catalog";
import type { RoutingTarget } from "./types";

export type CodexModelsUnavailableReason =
  | "codex_models_collision"
  | "codex_models_pagination_invalid"
  | "codex_models_probe_failed"
  | "codex_models_response_schema_changed";

export class CodexModelsUnavailableError extends Error {
  constructor(readonly reason: CodexModelsUnavailableReason) {
    super(reason);
    this.name = "CodexModelsUnavailableError";
  }
}

export interface CodexModelListPage {
  models: string[];
  nextCursor: string | null;
  itemCount: number;
}

export const MAX_CODEX_MODEL_LIST_PAGES = 128;
export const MAX_CODEX_MODEL_CURSOR_BYTES = 4_096;

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function identifier(value: unknown): string | undefined {
  if (typeof value !== "string" || value.length === 0 || value.length > 256
      || /[\u0000-\u001f\u007f]/.test(value)) return undefined;
  return value;
}

/**
 * Pin the stable Codex app-server v2 `model/list` authority fields. The
 * installed protocol declares `{ data, nextCursor }`; each item carries a
 * picker `id` and the executable `model`. Only `model` can become route
 * evidence. Display text and all other provider metadata are ignored.
 */
export function normalizeCodexModelListPage(value: unknown): CodexModelListPage {
  if (!record(value) || !Array.isArray(value.data)
      || (value.nextCursor !== null && typeof value.nextCursor !== "string")
      || (typeof value.nextCursor === "string"
        && Buffer.byteLength(value.nextCursor, "utf8") > MAX_CODEX_MODEL_CURSOR_BYTES)) {
    throw new CodexModelsUnavailableError("codex_models_response_schema_changed");
  }
  const models: string[] = [];
  for (const raw of value.data) {
    if (!record(raw) || typeof raw.hidden !== "boolean")
      throw new CodexModelsUnavailableError("codex_models_response_schema_changed");
    const id = identifier(raw.id);
    const model = identifier(raw.model);
    if (!id || !model)
      throw new CodexModelsUnavailableError("codex_models_response_schema_changed");
    if (!raw.hidden) models.push(model);
  }
  return { models, nextCursor: value.nextCursor, itemCount: value.data.length };
}

/** Project provider-controlled IDs onto Orchestration's exact OpenAI allowlist. */
export function normalizeCodexSupportedModels(
  advertisedModels: readonly string[],
  target: RoutingTarget,
  now = new Date(),
): ProviderModelObservation {
  if (target.provider !== "openai" || (target.authMode === "isolated" && !target.profile))
    throw new CodexModelsUnavailableError("codex_models_response_schema_changed");
  const exactModels = new Set<string>();
  for (const advertised of advertisedModels) {
    if (!providerSupportsModel("openai", advertised)
        || resolveModelAlias("openai", advertised) !== advertised) continue;
    if (exactModels.has(advertised))
      throw new CodexModelsUnavailableError("codex_models_collision");
    exactModels.add(advertised);
  }
  const authMode = target.authMode ?? "ambient";
  return {
    provider: "openai",
    targetId: target.id,
    authMode,
    ...(authMode === "isolated" ? { profile: target.profile } : {}),
    observedAt: now.toISOString(),
    source: CODEX_MODEL_OBSERVATION_SOURCE,
    models: [...exactModels].sort(),
  };
}
