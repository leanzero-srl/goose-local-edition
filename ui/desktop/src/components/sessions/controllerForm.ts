export interface ControllerFormField {
  name: string;
  title: string;
  description?: string;
  type: 'string' | 'number' | 'integer' | 'boolean';
  required: boolean;
  enum?: Array<string | number | boolean>;
  default?: string | number | boolean;
  minLength?: number;
  maxLength?: number;
  minimum?: number;
  maximum?: number;
  exclusiveMinimum?: number;
  exclusiveMaximum?: number;
}
export const UNSUPPORTED_FORM = 'This form needs a newer controller. Open it on the owner Mac.';
export function objectValue(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}
function onlyKeys(value: Record<string, unknown>, allowed: string[]): void {
  if (Object.keys(value).some((key) => !allowed.includes(key))) throw new Error(UNSUPPORTED_FORM);
}
function validateField(field: ControllerFormField, value: unknown): void {
  const { type, title } = field;
  if (
    (type === 'string' && typeof value !== 'string') ||
    (type === 'boolean' && typeof value !== 'boolean') ||
    ((type === 'number' || type === 'integer') &&
      (typeof value !== 'number' || !Number.isFinite(value))) ||
    (type === 'integer' && !Number.isSafeInteger(value))
  ) {
    throw new Error(`${title}: enter a valid ${type}.`);
  }
  if (field.enum && !field.enum.includes(value as string | number | boolean))
    throw new Error(`${title}: choose a supplied value.`);
  if (typeof value === 'string') {
    const length = [...value].length;
    if (field.minLength !== undefined && length < field.minLength)
      throw new Error(`${title}: minimum length is ${field.minLength}.`);
    if (field.maxLength !== undefined && length > field.maxLength)
      throw new Error(`${title}: maximum length is ${field.maxLength}.`);
  }
  if (typeof value === 'number') {
    if (field.minimum !== undefined && value < field.minimum)
      throw new Error(`${title}: minimum is ${field.minimum}.`);
    if (field.maximum !== undefined && value > field.maximum)
      throw new Error(`${title}: maximum is ${field.maximum}.`);
    if (field.exclusiveMinimum !== undefined && value <= field.exclusiveMinimum)
      throw new Error(`${title}: must exceed ${field.exclusiveMinimum}.`);
    if (field.exclusiveMaximum !== undefined && value >= field.exclusiveMaximum)
      throw new Error(`${title}: must be below ${field.exclusiveMaximum}.`);
  }
}
/** Unsupported constraints must stay visible and unanswerable, never silently disappear. */
export function controllerForm(schema: unknown): ControllerFormField[] {
  const root = objectValue(schema);
  const properties = objectValue(root?.properties);
  if (!root || root.type !== 'object' || !properties) throw new Error(UNSUPPORTED_FORM);
  onlyKeys(root, [
    '$schema',
    '$id',
    'title',
    'description',
    'type',
    'properties',
    'required',
    'additionalProperties',
  ]);
  if (root.additionalProperties !== undefined && typeof root.additionalProperties !== 'boolean')
    throw new Error(UNSUPPORTED_FORM);
  const required = root.required ?? [];
  if (
    !Array.isArray(required) ||
    required.some(
      (key) => typeof key !== 'string' || !Object.prototype.hasOwnProperty.call(properties, key)
    ) ||
    new Set(required).size !== required.length
  )
    throw new Error(UNSUPPORTED_FORM);
  return Object.entries(properties).map(([name, raw]) => {
    const value = objectValue(raw);
    if (!value || !['string', 'number', 'integer', 'boolean'].includes(String(value.type)))
      throw new Error(UNSUPPORTED_FORM);
    onlyKeys(value, [
      'type',
      'title',
      'description',
      'enum',
      'default',
      'minLength',
      'maxLength',
      'minimum',
      'maximum',
      'exclusiveMinimum',
      'exclusiveMaximum',
    ]);
    if (
      (value.title !== undefined && typeof value.title !== 'string') ||
      (value.description !== undefined && typeof value.description !== 'string')
    )
      throw new Error(UNSUPPORTED_FORM);
    for (const key of ['minLength', 'maxLength']) {
      if (
        value[key] !== undefined &&
        (value.type !== 'string' || !Number.isSafeInteger(value[key]) || Number(value[key]) < 0)
      )
        throw new Error(UNSUPPORTED_FORM);
    }
    for (const key of ['minimum', 'maximum', 'exclusiveMinimum', 'exclusiveMaximum']) {
      if (
        value[key] !== undefined &&
        (!['number', 'integer'].includes(String(value.type)) ||
          typeof value[key] !== 'number' ||
          !Number.isFinite(value[key]))
      )
        throw new Error(UNSUPPORTED_FORM);
    }
    const field = {
      ...value,
      name,
      title: value.title ?? name,
      required: required.includes(name),
    } as ControllerFormField;
    if (field.enum !== undefined) {
      if (
        !Array.isArray(field.enum) ||
        field.enum.length === 0 ||
        new Set(field.enum).size !== field.enum.length
      )
        throw new Error(UNSUPPORTED_FORM);
      for (const option of field.enum) validateField(field, option);
    }
    if (field.default !== undefined) validateField(field, field.default);
    return field;
  });
}
export function formAnswer(fields: ControllerFormField[], drafts: Record<string, string>): string {
  const result: Array<[string, unknown]> = [];
  for (const field of fields) {
    if (!Object.prototype.hasOwnProperty.call(drafts, field.name)) {
      if (field.required) throw new Error(`${field.title}: this field is required.`);
      continue;
    }
    const draft = drafts[field.name];
    let value: unknown = draft;
    if (field.enum) {
      const index = Number(draft);
      if (draft === '' || !Number.isInteger(index) || index < 0 || index >= field.enum.length)
        throw new Error(`${field.title}: choose a supplied value.`);
      value = field.enum[index];
    } else if (field.type !== 'string') {
      try {
        value = JSON.parse(draft);
      } catch {
        throw new Error(`${field.title}: enter a valid ${field.type}.`);
      }
    }
    validateField(field, value);
    result.push([field.name, value]);
  }
  return JSON.stringify(Object.fromEntries(result));
}
