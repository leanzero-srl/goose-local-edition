import { describe, expect, it } from "vitest";
import { constantTimeEqual, generateOtp, hashOtp } from "../src/lib/otp";

describe("generateOtp", () => {
  it("always returns exactly six digits", () => {
    for (let i = 0; i < 500; i++) {
      expect(generateOtp()).toMatch(/^[0-9]{6}$/);
    }
  });

  it("produces varying codes", () => {
    const codes = new Set(Array.from({ length: 50 }, () => generateOtp()));
    expect(codes.size).toBeGreaterThan(1);
  });
});

describe("hashOtp", () => {
  it("is deterministic for the same email and code", async () => {
    const a = await hashOtp("user@example.com", "123456");
    const b = await hashOtp("user@example.com", "123456");
    expect(a).toBe(b);
    expect(a).toMatch(/^[0-9a-f]{64}$/);
  });

  it("differs across codes and across emails", async () => {
    const base = await hashOtp("user@example.com", "123456");
    expect(await hashOtp("user@example.com", "123457")).not.toBe(base);
    expect(await hashOtp("other@example.com", "123456")).not.toBe(base);
  });
});

describe("constantTimeEqual", () => {
  it("matches equal strings", () => {
    expect(constantTimeEqual("abcdef", "abcdef")).toBe(true);
  });

  it("rejects different strings of equal length", () => {
    expect(constantTimeEqual("abcdef", "abcdeg")).toBe(false);
  });

  it("rejects different lengths", () => {
    expect(constantTimeEqual("abc", "abcd")).toBe(false);
  });
});
