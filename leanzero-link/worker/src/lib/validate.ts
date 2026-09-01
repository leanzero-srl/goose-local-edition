const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export function normalizeEmail(raw: unknown): string | null {
  if (typeof raw !== "string") {
    return null;
  }
  const email = raw.trim().toLowerCase();
  if (email.length === 0 || email.length > 254 || !EMAIL_PATTERN.test(email)) {
    return null;
  }
  return email;
}

export function normalizeCode(raw: unknown): string | null {
  if (typeof raw !== "string") {
    return null;
  }
  const code = raw.trim();
  return /^[0-9]{6}$/.test(code) ? code : null;
}
