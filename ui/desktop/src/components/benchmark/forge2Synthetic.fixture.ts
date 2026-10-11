import { execFileSync } from 'node:child_process';
import path from 'node:path';

interface SyntheticVerdict {
  score: number;
  scorerVersion: string;
  family: string;
  checks: Array<{ check: string; tier: string; score: number; detail: string }>;
  tiers: Record<string, { mean: number; weight: number; checks: number }>;
  rawScore: number;
  inner: number;
  critical: { multiplier: number; pre_severity_score: number; rows: unknown[] };
  reliability: {
    multiplier: number;
    defects: Array<{ tier: string; check: string; score: number; factor: number }>;
  };
  [key: string]: unknown;
}

/** SYNTHETIC composition controls, never native app or model results. Derive the current registry and
 *  every recorded number from the actual bundled scorer's pure APIs, with no probe or build. */
export const forge2Synthetic = JSON.parse(
  execFileSync(
    'python3',
    [
      '-B',
      '-c',
      `import json
import score_forge2 as sf
cases = {
    'groups': {'u_widget_numbers': .875, 'u_widget_chart': .875, 'u_ledger_table': .9712,
               'u_widget_live': 0, 'a_action_result': .6, 'r9_widget_boot': .6667, 'e_event_economy': .5},
    'fractional': {'t_trigger_handoff': .999, 'u_widget_numbers': .875,
                   'r7_values_fresh': .888, 'r7_writes_accepted': .9759},
    'jql_membership': {'r10_initial_membership': .5, 'r10_shared_permissions': 0},
}
verdicts = {}
for name, overrides in cases.items():
    assert set(overrides) <= sf.REGISTERED
    parts = {'r10_shared_permissions': {'critical_observed': False}} if name == 'jql_membership' else None
    rows = sf._scenario(overrides, parts)
    if name == 'jql_membership':
        next(r for r in rows if r['check'] == 'r10_shared_permissions')['detail'] = 'JQL disclosure not observed: synthetic membership failure'
    verdicts[name] = sf.compose_from_rows(rows)
print(json.dumps({'nativeScoringPerformed': False, 'paidModelCalls': 0,
                  'method': 'score_forge2._scenario -> compose_from_rows; no native probe',
                  'scorerFilesSha256': sf.scorer_files_sha256(),
                  'tierOrder': list(sf.TIER_ORDER),
                  'registry': [{'check': n, 'tier': t} for n, t, *_ in sf.CHECKS],
                  'verdicts': verdicts}))`,
    ],
    {
      cwd: path.resolve(__dirname, '../../../../../evals/swarm-bench/bench'),
      encoding: 'utf8',
    }
  )
) as {
  nativeScoringPerformed: false;
  paidModelCalls: 0;
  method: string;
  scorerFilesSha256: Record<string, string>;
  tierOrder: string[];
  registry: Array<{ check: string; tier: string }>;
  verdicts: Record<'groups' | 'fractional' | 'jql_membership', SyntheticVerdict>;
};
