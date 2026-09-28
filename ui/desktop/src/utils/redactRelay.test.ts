import { describe, expect, it } from 'vitest';
import FIXTURE from '../../../../crates/goose-provider-types/src/redact.fixture.json';
import { redactRelayCapability } from './redactRelay';

/**
 * The Rust rule (`redact.rs` `the_shared_fixture_holds_for_the_rust_rule`) runs these same cases:
 * the fixture is what keeps goose's two ports of the relay-capability rule one rule.
 */
describe('redact.fixture.json — the desktop’s relay rule, pinned to goose’s', () => {
  it('has the cases both ports are held to', () => {
    expect(FIXTURE.cases.length).toBeGreaterThanOrEqual(20);
  });

  for (const c of FIXTURE.cases) {
    it(c.name, () => {
      expect(redactRelayCapability(c.input)).toBe(c.output);
      expect(redactRelayCapability(c.output)).toBe(c.output);
    });
  }
});
