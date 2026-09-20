/** Model identifiers are argv values, never filesystem names or shell fragments. */
export function validCloudEntrant(value: { provider: unknown; model: unknown }): boolean {
  return (
    typeof value.provider === 'string' &&
    /^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(value.provider) &&
    typeof value.model === 'string' &&
    value.model.length > 0 &&
    !value.model.startsWith('-') &&
    [...value.model].every((char) => char.charCodeAt(0) > 32 && char.charCodeAt(0) !== 127)
  );
}
