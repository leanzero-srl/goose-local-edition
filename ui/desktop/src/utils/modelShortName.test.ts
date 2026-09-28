import { describe, expect, it } from 'vitest';
import fixture from './modelShortName.fixture.json';
import { modelShortName } from './modelShortName';

describe('modelShortName (Q-308)', () => {
  it('is the rule goose-sidecar pins to the same fixture', () => {
    for (const [repo, short] of fixture.cases) {
      expect(modelShortName(repo), repo).toBe(short);
    }
  });
});
