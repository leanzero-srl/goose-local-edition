import { describe, expect, it } from "vitest";
import { parseHujson, stripHujson } from "../src/lib/hujson";

describe("parseHujson", () => {
  it("parses plain JSON unchanged", () => {
    expect(parseHujson('{"a":[1,2,{"b":"c"}]}')).toEqual({ ok: true, value: { a: [1, 2, { b: "c" }] } });
  });

  it("strips line comments, block comments and trailing commas", () => {
    const text = `// header
{
  /* block
     comment */ "acls": [
    { "action": "accept", "src": ["*"], "dst": ["autogroup:self:*"], }, // trailing
  ],
}`;
    expect(parseHujson(text)).toEqual({
      ok: true,
      value: { acls: [{ action: "accept", src: ["*"], dst: ["autogroup:self:*"] }] },
    });
  });

  it("leaves comment-looking and comma sequences inside strings alone", () => {
    const text = '{"url": "https://x.example/a//b", "note": "/* not a comment */", "list": ["a,", "b"]}';
    expect(parseHujson(text)).toEqual({
      ok: true,
      value: { url: "https://x.example/a//b", note: "/* not a comment */", list: ["a,", "b"] },
    });
  });

  it("handles escaped quotes inside strings", () => {
    expect(parseHujson('{"q": "say \\"hi\\" // still string", }')).toEqual({ ok: true, value: { q: 'say "hi" // still string' } });
  });

  it("reports an unterminated block comment or string instead of guessing", () => {
    expect(stripHujson('{"a": 1 /* never closed').ok).toBe(false);
    expect(stripHujson('{"a": "never closed').ok).toBe(false);
  });

  it("reports what is still not JSON after stripping", () => {
    const result = parseHujson('{"acls": [ {"action": "accept", "src": ["*"');
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error.length).toBeGreaterThan(0);
    }
  });
});
