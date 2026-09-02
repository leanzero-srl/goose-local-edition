import { describe, it, expect } from 'vitest';
import { benchRunArgvTokens, pidsMatchingTokens } from '../benchReap';

const WD = '/Users/x/goose-builds/sb7-r9';
const PS = [
  `  4101 python3 -u run_build.py --entrant fleet --only-rep 0 --timeout 0 --out ${WD}`,
  `  4102 /Applications/Goose.app/Contents/Resources/bin/goose swarm run --log-file ${WD}/run.jsonl --spec spec.md`,
  `  4103 python3 vendorsync_scorer.py --db ${WD}/graded.db --port 8850`,
  `  4104 tail -f ${WD}/run.jsonl`,
  `  4105 less ${WD}/.swarm/activity/t1.log`,
  `  4106 python3 tick.py ${WD}`,
  `  4107 /Applications/Goose.app/Contents/Resources/bin/goose swarm run --log-file /Users/x/other/run.jsonl`,
  `  4108 /Applications/Goose.app/Contents/MacOS/Goose --remote-debugging-port=9897`,
  // sb-7 tier (score_sb7.py _spawn_ledgerd/_spawn_notifierd; run_build.py:291 db = <W>/graded-sb7-db)
  `  4109 /usr/bin/python3 -m app.ledgerd --db-dir ${WD}/graded-sb7-db --port 8123 --notifier http://127.0.0.1:8124 --vendor http://127.0.0.1:8850 --tokens-file ${WD}/tokens.json`,
  `  4110 /usr/bin/python3 -m app.notifierd --db-dir ${WD}/graded-sb7-db --port 8124`,
  `  4111 /usr/bin/python3 -m app.ledgerd --db-dir ${WD}/sb7-empty-db --port 8125 --notifier http://127.0.0.1:8126 --vendor http://127.0.0.1:8850 --tokens-file ${WD}/tokens.json`,
  `  4112 /usr/bin/python3 -m app.notifierd --db-dir ${WD}/sb7-combined-db --port 8127`,
  // another run's sb-7 app, and a SIBLING workdir sharing this one's name as a prefix
  `  4113 /usr/bin/python3 -m app.ledgerd --db-dir /Users/x/other/graded-sb7-db --port 8130`,
  `  4114 /usr/bin/python3 -m app.ledgerd --db-dir ${WD}b/graded-sb7-db --port 8131`,
  // the desktop's own goose serve (gooseServe.ts argv) must never match
  `  4115 /Applications/Goose.app/Contents/Resources/bin/goose serve --platform desktop --host 127.0.0.1 --port 52341`,
].join('\n');

describe('benchmark cancel reaps by run-unique argv tokens, per pid', () => {
  it('names the engine log and the db PATH PREFIXES only this run carries', () => {
    expect(benchRunArgvTokens(WD)).toEqual([
      `--log-file ${WD}/run.jsonl`,
      `${WD}/graded`,
      `${WD}/sb7-empty-db`,
      `${WD}/sb7-combined-db`,
    ]);
  });

  it('sb-6 shape: matches the engine and the `--db <W>/graded.db` scorer child, NOT tail/less/tick.py', () => {
    const sb6 = PS.split('\n').slice(0, 8).join('\n');
    expect(pidsMatchingTokens(sb6, benchRunArgvTokens(WD), 4108)).toEqual([4102, 4103]);
  });

  it('sb-7 shape: matches ledgerd/notifierd on `--db-dir <W>/graded-sb7-db` and the empty/combined instances', () => {
    expect(pidsMatchingTokens(PS, benchRunArgvTokens(WD), 4108)).toEqual([
      4102, 4103, 4109, 4110, 4111, 4112,
    ]);
  });

  it('never matches another run, a sibling workdir sharing the prefix, goose serve, or the caller', () => {
    const pids = pidsMatchingTokens(PS, benchRunArgvTokens(WD), 4108);
    expect(pids).not.toContain(4113);
    expect(pids).not.toContain(4114);
    expect(pids).not.toContain(4115);
    expect(pidsMatchingTokens(PS, benchRunArgvTokens('/Users/x/other'), 4108)).toEqual([4107, 4113]);
    expect(pidsMatchingTokens(PS, benchRunArgvTokens(WD), 4102)).not.toContain(4102);
  });

  it('ignores lines that are not `pid args`', () => {
    expect(pidsMatchingTokens('garbage\n\n  PID ARGS\n', benchRunArgvTokens(WD), 1)).toEqual([]);
  });
});
