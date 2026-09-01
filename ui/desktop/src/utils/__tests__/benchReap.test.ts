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
].join('\n');

describe('benchmark cancel reaps by run-unique argv tokens, per pid', () => {
  it('names the two tokens only this run carries', () => {
    expect(benchRunArgvTokens(WD)).toEqual([
      `--log-file ${WD}/run.jsonl`,
      `--db ${WD}/graded.db`,
    ]);
  });

  it('matches the engine and the scorer child, and NOT tail/less/tick.py holding the same path', () => {
    expect(pidsMatchingTokens(PS, benchRunArgvTokens(WD), 4108)).toEqual([4102, 4103]);
  });

  it('never matches another run, and never the caller itself', () => {
    expect(pidsMatchingTokens(PS, benchRunArgvTokens('/Users/x/other'), 4108)).toEqual([4107]);
    expect(pidsMatchingTokens(PS, benchRunArgvTokens(WD), 4102)).toEqual([4103]);
  });

  it('ignores lines that are not `pid args`', () => {
    expect(pidsMatchingTokens('garbage\n\n  PID ARGS\n', benchRunArgvTokens(WD), 1)).toEqual([]);
  });
});
