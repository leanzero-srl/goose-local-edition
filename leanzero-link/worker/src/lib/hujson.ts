// HuJSON ("human JSON", https://github.com/tailscale/hujson — the format Tailscale and
// Headscale policies are written in) is JSON plus `//` line comments, `/* */` block
// comments and trailing commas before `]` / `}`. Headscale stores and returns the policy
// text VERBATIM (hscontrol/grpcv1.go GetPolicy, PolicyModeDB), so a hand-written policy
// with comments comes back with them and plain JSON.parse fails on it. This strips the
// three extensions outside string literals and hands the rest to JSON.parse.

export type HujsonParse = { ok: true; value: unknown } | { ok: false; error: string };

function skipTrivia(text: string, start: number): number {
  let i = start;
  for (;;) {
    const c = text[i];
    if (c === " " || c === "\t" || c === "\n" || c === "\r") {
      i++;
      continue;
    }
    if (c === "/" && text[i + 1] === "/") {
      const end = text.indexOf("\n", i + 2);
      i = end === -1 ? text.length : end + 1;
      continue;
    }
    if (c === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      if (end === -1) {
        return -1;
      }
      i = end + 2;
      continue;
    }
    return i;
  }
}

export function stripHujson(text: string): { ok: true; json: string } | { ok: false; error: string } {
  let out = "";
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') {
        j += text[j] === "\\" ? 2 : 1;
      }
      if (j >= text.length) {
        return { ok: false, error: `unterminated string at offset ${i}` };
      }
      out += text.slice(i, j + 1);
      i = j + 1;
      continue;
    }
    if (c === "/" && (text[i + 1] === "/" || text[i + 1] === "*")) {
      const next = skipTrivia(text, i);
      if (next === -1) {
        return { ok: false, error: `unterminated block comment at offset ${i}` };
      }
      out += " ";
      i = next;
      continue;
    }
    if (c === ",") {
      const next = skipTrivia(text, i + 1);
      if (next === -1) {
        return { ok: false, error: `unterminated block comment after offset ${i}` };
      }
      const following = text[next];
      if (following === "]" || following === "}") {
        i++;
        continue;
      }
    }
    out += c;
    i++;
  }
  return { ok: true, json: out };
}

export function parseHujson(text: string): HujsonParse {
  const stripped = stripHujson(text);
  if (!stripped.ok) {
    return stripped;
  }
  try {
    return { ok: true, value: JSON.parse(stripped.json) };
  } catch (error) {
    return { ok: false, error: error instanceof Error ? error.message : String(error) };
  }
}
