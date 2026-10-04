/**
 * The per-model custom fields a single-model benchmark run sends and records. The Benchmark form
 * reads the model's saved values (ModelCustomFields); main hands them to the entrant and stamps
 * them on the result, so a score says which effort and sampling it ran at.
 *
 * Wire: main sets GOOSE_MODEL_FIELDS = {provider: {model: values}} on run_build.py; the
 * `goose benchmark-config` it runs reads that variable (never the saved config.yaml map) and puts
 * the provider's slice in the entrant's isolated config, where goose applies it to that model's
 * requests (crates/goose/src/model_fields.rs). A run launched without the variable carries none.
 */

export const MODEL_FIELDS_ENV = 'GOOSE_MODEL_FIELDS';
export const EFFORT_FIELD = 'effort';

export type BenchModelFields = Record<string, string | number>;

/** What a result records: the values the entrant was sent, and the fields the tier pinned instead. */
export interface BenchModelFieldsRecord {
  sent: BenchModelFields;
  /** Field ids the tier fixes for every entrant (Forge pins the reasoning effort). */
  pinnedByTier: string[];
}

const FIELD_ID = /^[a-z][a-z0-9_]*$/;

/** A field map from the renderer: ids are snake_case names, values strings or finite numbers. */
export function validBenchModelFields(value: unknown): value is BenchModelFields {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return false;
  return Object.entries(value).every(
    ([id, field]) =>
      FIELD_ID.test(id) &&
      ((typeof field === 'string' && field.length > 0 && field.length <= 64) ||
        (typeof field === 'number' && Number.isFinite(field)))
  );
}

/** The fields a tier pins, with the reason the form shows: a pinned effort is the tier's, not the
 *  model's, so the run never sends the saved one. */
export function forgeEffortPin(effort: string | null | undefined): Record<string, string> {
  return {
    [EFFORT_FIELD]: effort
      ? `Forge runs every model at reasoning effort ${effort}`
      : 'Forge runs every model at one pinned reasoning effort',
  };
}

/** Splits the saved values into what the run sends and what the tier pins. */
export function modelFieldsForRun(
  values: BenchModelFields,
  tierPinsEffort: boolean
): BenchModelFieldsRecord {
  const pinnedByTier = tierPinsEffort ? [EFFORT_FIELD] : [];
  const sent = Object.fromEntries(
    Object.entries(values).filter(([id]) => !pinnedByTier.includes(id))
  );
  return { sent, pinnedByTier };
}

/** The variable run_build.py's benchmark-config reads. Always set for a single-model run — `{}`
 *  when it sends nothing — so neither an inherited variable nor (on a tier that does not isolate
 *  the entrant's config) the saved config.yaml map can add a field the result does not record. */
export function modelFieldsLaunchEnv(
  provider: string,
  model: string,
  sent: BenchModelFields
): Record<string, string> {
  const payload = Object.keys(sent).length === 0 ? {} : { [provider]: { [model]: sent } };
  return { [MODEL_FIELDS_ENV]: JSON.stringify(payload) };
}

/** "effort low · top_k 40", or what the run used instead. */
export function describeRunModelFields(record: BenchModelFieldsRecord | null | undefined): string {
  if (!record) return 'not recorded (the run predates this record)';
  const sent = Object.keys(record.sent)
    .sort()
    .map((id) => `${id} ${record.sent[id]}`)
    .join(' · ');
  const parts = [sent || "the model's defaults"];
  if (record.pinnedByTier.length > 0)
    parts.push(`${record.pinnedByTier.join(', ')} pinned by the tier`);
  return parts.join('; ');
}
