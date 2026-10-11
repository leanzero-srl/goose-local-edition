import { describe, expect, it } from 'vitest';
import { forgeTaskFingerprint, forgeTaskPublicationFields, forgeTaskPublicationProblem } from './benchForgeTask';

describe('Forge 2.0 publication task receipt', () => {
  const raw = Buffer.from('{"scorerVersion":"forge-2.0","task":"current"}\n');
  const fingerprint = forgeTaskFingerprint(raw);

  it('hashes raw installed bytes: JSON whitespace is part of the frozen artifact identity', () => {
    expect(fingerprint).toBe('fca21dde56491b36b4b985c6d6f73d120ecdcaf4bf2e012f58b36612da0cb865');
    expect(forgeTaskFingerprint(Buffer.from(raw.toString().trim()))).not.toBe(fingerprint);
  });

  it('publishes the captured build receipt and refuses to borrow one from a newer installed task', () => {
    const stored = { scorerVersion: 'forge-2.0', taskFingerprint: fingerprint };
    expect(forgeTaskPublicationProblem(stored, fingerprint)).toBeNull();
    expect(forgeTaskPublicationFields(stored)).toEqual({ taskFingerprint: fingerprint });
    expect(forgeTaskPublicationProblem(stored, forgeTaskFingerprint(Buffer.from('new task')))).toMatch(/different Forge 2.0 task/);
    expect(forgeTaskPublicationProblem({ scorerVersion: 'forge-2.0' }, fingerprint)).toMatch(/no recorded task receipt/);
    expect(() => forgeTaskPublicationFields({ scorerVersion: 'forge-2.0' })).toThrow(/no recorded task receipt/);
    expect(forgeTaskPublicationProblem(stored, 'unreadable')).toMatch(/different Forge 2.0 task/);
  });

  it('leaves Forge 1.0 and Gauntlet publication unchanged', () => {
    for (const scorerVersion of ['forge-1.0', 'sb-7.2']) {
      expect(forgeTaskPublicationProblem({ scorerVersion }, 'unreadable')).toBeNull();
      expect(forgeTaskPublicationFields({ scorerVersion })).toEqual({});
    }
  });
});
