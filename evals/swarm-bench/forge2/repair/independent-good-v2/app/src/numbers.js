// Estimates are summed in integer micro-units so 0.1 + 0.2 stays 0.3 and the creep rounding below works
// on exact integers rather than on binary floats.
const MICRO = 1_000_000;

export const toMicro = (value) => {
  const n = typeof value === 'number' ? value : value === null || value === undefined || value === '' ? 0 : Number(value);
  return Number.isFinite(n) ? Math.round(n * MICRO) : 0;
};

export const fromMicro = (micro) => micro / MICRO;

// "plain decimals (34.5, 0), no thousands separators"
export const formatPoints = (micro) => {
  const n = fromMicro(micro);
  return Object.is(n, -0) ? '0' : String(n);
};

// creep = 100 × added / committed, rounded half away from zero to one decimal. Returns the value in
// tenths of a percent as an integer, or null when committed is 0.
export function creepTenths(addedMicro, committedMicro) {
  if (committedMicro === 0) return null;
  const num = 1000n * BigInt(addedMicro);
  const den = BigInt(committedMicro);
  const sign = (num < 0n) !== (den < 0n) ? -1n : 1n;
  const a = num < 0n ? -num : num;
  const b = den < 0n ? -den : den;
  const tenths = (2n * a + b) / (2n * b);
  return Number(sign * tenths);
}

export const creepPercent = (tenths) => (tenths === null ? null : tenths / 10);

export function formatCreep(tenths) {
  if (tenths === null) return '—';
  const sign = tenths < 0 ? '-' : '';
  const abs = Math.abs(tenths);
  return `${sign}${Math.trunc(abs / 10)}.${abs % 10}%`;
}
