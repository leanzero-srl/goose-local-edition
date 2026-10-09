'use strict';
// The SERVER half of `forge lint`, offline. Real `forge lint` runs its client linters (the kit's bin/lint.cjs runs the
// same 16, byte for byte) AND uploads the interpolated manifest.yml to `appPreDeploymentCheck`; every server outcome
// {rule, category, reason} is printed at `manifest.yml 0:0` with the rule as its reference (@forge/lint 6.3.0
// server-side-linter.js mapServerSideResponse). No offline package carries those rules, so they are kept here.
//
// ONLY MEASURED REFUSALS. Each rule below was triggered on the real server by a one-change variant of a valid manifest
// and the message is the one it printed (forge CLI 14.1.0 on wolfaenpak, 2026-10-09; forge2/research/understand/
// real-forge-fidelity.md §2.3, variants v01-v10). Documented constraints the server did NOT refuse at lint — two
// indexes with one name (v04), an `any`-typed range (v05) or partition (v06) attribute — are deliberately absent:
// refusing them here would fail an app that deploys.
//
//   rules(manifest) -> [{rule, category, reason, variant}]   (empty when the manifest passes)

const RULE = 'MANIFEST_INVALID_RULE';

// The CLI's feature flag `xls-forge-cli-deprecated-runtimes`, fetched 2026-10-09 with the CLI's own request shape
// (real-forge-fidelity.md §2.4a). The client RuntimeVersionValidator turns it into a `deprecated-property` WARNING;
// offline, the CLI itself would silently use [] instead.
const DEPRECATED_RUNTIMES = ['sandbox', 'nodejs18.x', 'nodejs20.x'];

const MESSAGES = {
  rangeCount: 'Storage entity named index must include exactly one range attribute.', // v01 range [at, changeId]
  nameShort: 'Storage entity index name is too short.', // v02 `bs` (2 chars); `by-sprint` (9) passes
  nameLong: 'Storage entity index name is too long.', // v10 51 chars
  nameChars: 'Storage entity index name contains non-allowed characters.', // v03 `by sprint`
  entityCount: 'Your app exceeds the maximum number of custom entities allowed: 20', // v07 21 entities
  node20: 'The nodejs20.x runtime is deprecated. Migrate your app to the latest Node.js runtime: https://go.atlassian.com/runtime', // v08
};
const MAX_ENTITIES = 20; // measured: 21 refused (message above)
const INDEX_NAME_MIN = 3; // measured: 2 refused
const INDEX_NAME_MAX = 50; // measured: 51 refused

function rules(manifest) {
  const out = [];
  const add = (key, variant) => {
    if (!out.some((f) => f.reason === MESSAGES[key])) out.push({ rule: RULE, category: 'ERROR', reason: MESSAGES[key], variant });
  };
  const entities = manifest?.app?.storage?.entities;
  if (Array.isArray(entities)) {
    if (entities.length > MAX_ENTITIES) add('entityCount', 'v07');
    for (const entity of entities) {
      for (const ix of Array.isArray(entity?.indexes) ? entity.indexes : []) {
        // The string form (`indexes: [at]`) passed the server (v09); these rules are about NAMED (object) indexes.
        if (!ix || typeof ix !== 'object') continue;
        if (Array.isArray(ix.range) && ix.range.length > 1) add('rangeCount', 'v01');
        if (typeof ix.name !== 'string') continue;
        if (ix.name.length < INDEX_NAME_MIN) add('nameShort', 'v02');
        if (ix.name.length > INDEX_NAME_MAX) add('nameLong', 'v10');
        // Measured with a space only; other characters are unmeasured and accepted (lenient, never a false refusal).
        if (/\s/.test(ix.name)) add('nameChars', 'v03');
      }
    }
  }
  if (manifest?.app?.runtime?.name === 'nodejs20.x') add('node20', 'v08');
  return out;
}

module.exports = { rules, DEPRECATED_RUNTIMES, MESSAGES, RULE };
