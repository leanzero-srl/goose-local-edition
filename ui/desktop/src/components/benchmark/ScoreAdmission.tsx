import { Chip, TYPE, TNUM, cx } from '../lz';

export interface Admission {
  visible: boolean;
  matching: boolean;
  good: boolean;
  excellence: { visual: boolean; backend: boolean };
  ceiling: number;
  reasons: string[];
}

export function ScoreAdmission({
  admission,
  rawScore,
  score,
}: {
  admission?: Admission;
  rawScore?: number;
  score: number;
}) {
  if (!admission || typeof rawScore !== 'number')
    return (
      <p role="status" className={TYPE.bodyMuted}>
        This SB7.1 result is missing its score admission evidence.
      </p>
    );
  const gates = [
    ['Visible payment scene', admission.visible],
    ['Required structure and data mapping', admission.matching],
    ['Presentation and animation quality', admission.good],
    ['Visual excellence', admission.excellence.visual],
    ['Backend excellence', admission.excellence.backend],
  ] as const;
  return (
    <section className="flex flex-col gap-3" aria-label="Score admission">
      <div className="flex flex-wrap gap-2">
        {gates.map(([label, passed]) => (
          <Chip key={label} tone={passed ? 'ok' : 'err'}>
            {label}: {passed ? 'passed' : 'not met'}
          </Chip>
        ))}
      </div>
      <dl className={cx('grid grid-cols-3 gap-3', TNUM)}>
        <div>
          <dt className={TYPE.meta}>Earned score before ceiling</dt>
          <dd>{rawScore.toFixed(3)}</dd>
        </div>
        <div>
          <dt className={TYPE.meta}>Admission ceiling</dt>
          <dd>{admission.ceiling.toFixed(3)}</dd>
        </div>
        <div>
          <dt className={TYPE.meta}>Final score</dt>
          <dd>{score.toFixed(3)}</dd>
        </div>
      </dl>
      <p className={TYPE.bodyMuted}>
        Final score is the lower of earned credit and the admission ceiling. Passing admission adds
        no points; visual and backend excellence must both pass for the highest band.
      </p>
      {admission.reasons.map((reason) => (
        <p key={reason} className={TYPE.body}>
          {reason}
        </p>
      ))}
    </section>
  );
}
