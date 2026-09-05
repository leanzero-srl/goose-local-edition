// ── web/viz.js · section 7 (Labels) · piece: amount formatting ──────────────
// Text for #viz-labels. Formats one record's integer minor units in its OWN
// currency; exponent from CURRENCY_EXP (section-1 constant, owned by the
// constants shard). Never sums or mixes currencies.

function fmtGroupDigits(intVal) {
  const neg = intVal < 0;
  const s = String(Math.abs(Math.trunc(intVal)));
  const first = s.length % 3 || 3;
  let out = s.slice(0, first);
  for (let i = first; i < s.length; i += 3) out += ',' + s.slice(i, i + 3);
  return (neg ? '-' : '') + out;
}

function formatAmount(amountMinor, currency) {
  const exp = Object.prototype.hasOwnProperty.call(CURRENCY_EXP, currency)
    ? CURRENCY_EXP[currency]
    : 2; // fallback only; fixture codes are exactly the four in CURRENCY_EXP
  const pow = Math.pow(10, exp);
  const intPart = Math.trunc(amountMinor / pow);
  let body;
  if (exp === 0) {
    body = fmtGroupDigits(intPart); // JPY: exponent 0, no decimals
  } else {
    const frac = amountMinor - intPart * pow; // exact integer remainder, no float drift
    body = fmtGroupDigits(intPart) + '.' + String(frac).padStart(exp, '0');
  }
  return currency + ' ' + body; // "EUR 1,299.00" · "JPY 58" · "KWD 46.700" · "USD 20.70"
}
