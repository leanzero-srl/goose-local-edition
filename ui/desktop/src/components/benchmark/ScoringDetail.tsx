import { ScoreAdmission, type Admission } from './ScoreAdmission';
import { useMemo, useState } from 'react';
import {
  FORGE_TIERS,
  FORGE_TIER_ORDER,
  SB8_TIERS,
  VERDICT_TIER_INFO as TIER_INFO,
  VERDICT_TIER_ORDER as TIER_ORDER,
  isForge,
  isSb8,
  isolatedPaymentsTier,
} from './baselines';
import { sb8CompositionSchema } from '../../sb8ScoreSchema';
import { Check, ChevronDown, ChevronRight, X, XCircle } from 'lucide-react';
import {
  Chip,
  FOCUS,
  MOTION,
  RADIUS,
  ROW,
  SPACE,
  SURFACE,
  SectionHeader,
  TNUM,
  TONE_FILL,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type Tone,
} from '../lz';

/**
 * The full scoring story behind the single number — the sb-5.2 composition formula with each
 * component's real contribution, every check the scorer ran with its evidence string verbatim,
 * the findings that survived the repair waves, and the round-by-round repair progression.
 *
 * Everything here is scorer/engine truth persisted with the result (main.ts `verdict`); nothing
 * is re-derived from a model. Studio tokens only: the status triad says pass/partial/fail, the
 * accent is what a build EARNED, tiers are told apart by their letter — never by a node hue.
 */

export interface VerdictCheck {
  check: string;
  tier: string;
  score: number;
  detail?: string;
  consequence?: string;
  parts?: Record<string, unknown>;
}

interface LegacyVerdictDetail {
  inner?: number;
  critical?: {
    multiplier: number;
    floor?: number;
    pre_severity_score?: number;
    rows: Array<{ check: string; factor?: number; why?: string; suppressed?: string }>;
  };
  excellence?: { fraction: number; e_mean: number; conditions: Record<string, unknown> };
  /** Forge (score_forge.py): the verdict's own publishability, runtime and calibration identity. */
  status?: string;
  publishable?: boolean;
  unpublishable_reasons?: string[];
  runtime?: string;
  calibration?: string;
  probe_unavailable?: unknown[];
  vacuous?: unknown[];
  harness_missing?: unknown[];
  sched_unreached?: unknown[];
  rawScore?: number;
  admission?: Admission | ForgeAdmissionRecord;
  checks: VerdictCheck[];
  tiers: Record<string, { mean: number; checks: number; weight: number; admission_only?: boolean }>;
  core?: number;
  hard?: number;
  excellent?: boolean;
  solid?: boolean;
  root_causes?: Record<string, string[]>;
  findingsHeld?: string[];
  repairRounds?: Array<{ round: number; findings: number }>;
}

export interface Sb8VerdictDetail {
  scorerVersion: string;
  checks: Array<{ name: string; tier: string; score: number; detail?: string }>;
  tiers: Record<string, number>;
  scoreInner: number;
  criticalMultiplier: number;
  calibrated?: boolean;
  weights?: Record<string, number>;
  core_tiers?: string[];
}

export type VerdictDetail = LegacyVerdictDetail | Sb8VerdictDetail;

// The six checks scored OUTSIDE their home tier as the standalone 10% hard block — mirrors the
// scorer's HARD_BLOCK so their rows can say so instead of silently not moving the tier mean.
const HARD_CHECKS = new Set([
  'request_efficiency',
  'second_sync_cost',
  'client_create_replay',
  'client_idempotency_key',
  'update_propagation',
  'restart_persistence',
]);

/** Full marks read ok, nothing reads err, anything between is the warn step. */
const scoreTone = (s: number): Tone => (s >= 1 ? 'ok' : s <= 0 ? 'err' : 'warn');

const humanize = (s: string) => {
  const t = s.replace(/_/g, ' ').trim();
  return t.charAt(0).toUpperCase() + t.slice(1);
};

const pct = (v: number, digits = 0) => `${(v * 100).toFixed(digits)}%`;

function ScoreChip({ score }: { score: number }) {
  return (
    <span
      data-testid="score-chip"
      data-tone={scoreTone(score)}
      className={cx(
        'inline-flex h-5 w-12 shrink-0 items-center justify-center text-lz-meta',
        WEIGHT.semibold,
        TNUM,
        RADIUS.control,
        TONE_FILL[scoreTone(score)]
      )}
    >
      {Math.round(score * 100)}
    </span>
  );
}

/** parts — the scorer's per-item evidence map. Booleans as solid check/cross chips, numbers inline. */
function PartChips({ parts }: { parts: Record<string, unknown> }) {
  const entries = Object.entries(parts).slice(0, 24);
  if (entries.length === 0) return null;
  return (
    <div className="mt-1.5 flex flex-wrap gap-1">
      {entries.map(([key, value]) => {
        if (typeof value === 'boolean') {
          return (
            <Chip key={key} tone={value ? 'ok' : 'err'} icon={value ? <Check /> : <X />}>
              {key}
            </Chip>
          );
        }
        const shown =
          typeof value === 'number'
            ? Number.isInteger(value)
              ? String(value)
              : value.toFixed(2)
            : String(value).slice(0, 40);
        return (
          <Chip key={key}>
            {key} {shown}
          </Chip>
        );
      })}
    </div>
  );
}

function CheckRow({ check }: { check: VerdictCheck }) {
  return (
    <div className={cx('flex items-start gap-3 border-t px-3 py-2.5', SURFACE.hairline)}>
      <ScoreChip score={check.score} />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className={cx(TYPE.body, WEIGHT.semibold)}>{humanize(check.check)}</span>
          {HARD_CHECKS.has(check.check) && (
            <Chip title="Scored in the standalone 10% hard block, not this tier's mean">
              hard block · 10%
            </Chip>
          )}
        </div>
        {check.detail && (
          <div className="mt-0.5 break-words font-mono text-lz-mono text-lz-ink-2">
            {check.detail}
          </div>
        )}
        {check.score < 1 && check.consequence && (
          <div className={cx('mt-0.5 text-lz-meta', WEIGHT.medium, TONE_TEXT.err)}>
            Costs: {check.consequence}
          </div>
        )}
        {check.parts && <PartChips parts={check.parts} />}
      </div>
    </div>
  );
}

/** One expandable tier group — custom accordion, no native details/summary. */
function TierGroup({
  tier,
  checks,
  mean,
  weight,
  admissionOnly = false,
  open,
  onToggle,
  sb8 = false,
  forge = false,
}: {
  tier: string;
  checks: VerdictCheck[];
  mean: number | null;
  weight: number | null;
  admissionOnly?: boolean;
  open: boolean;
  onToggle: () => void;
  sb8?: boolean;
  forge?: boolean;
}) {
  const info = (sb8 ? SB8_TIERS[tier] : forge ? FORGE_TIERS[tier] : TIER_INFO[tier]) ?? {
    name: tier,
    desc: '',
  };
  const lost = checks.filter((c) => c.score < 1).length;
  return (
    <div className={cx(SURFACE.card, 'overflow-hidden')}>
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={open}
        className={cx(
          'flex w-full items-center gap-3 px-3 text-left',
          ROW.default,
          SURFACE.hover,
          FOCUS,
          MOTION
        )}
      >
        {open ? (
          <ChevronDown className="size-4 shrink-0 text-lz-ink-3" />
        ) : (
          <ChevronRight className="size-4 shrink-0 text-lz-ink-3" />
        )}
        <Chip className="w-7 justify-center">{tier}</Chip>
        <span className="min-w-0 flex-1 truncate" title={info.desc || undefined}>
          <span className={cx(TYPE.body, WEIGHT.semibold)}>{info.name}</span>
          <span className={cx('ml-2 hidden sm:inline', TYPE.meta)}>{info.desc}</span>
        </span>
        {lost > 0 && (
          <Chip tone="err">
            {lost} lost point{lost > 1 ? 's' : ''}
          </Chip>
        )}
        <span className={cx('shrink-0', TYPE.meta, TNUM)}>
          {checks.length} checks
          {admissionOnly ? ' · admission gate' : weight != null ? ` · weight ${pct(weight)}` : ''}
        </span>
        {mean != null && <ScoreChip score={mean} />}
      </button>
      {open && (
        <div>
          {checks.map((c) => (
            <CheckRow key={c.check} check={c} />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * The sb-5.2 composition: score = 60% core (A 25 / B 30 / C 25 / D 20) + 15% journey + 10% visual
 * + 5% performance + 10% hard block. Rendered as a stacked contribution bar (each slot is a
 * component's maximum share of the 100; the accent fill is what this build actually earned) plus
 * the arithmetic, so the final number is reproducible by eye.
 */
function CompositionBar({ verdict, score }: { verdict: LegacyVerdictDetail; score: number }) {
  const t = verdict.tiers;
  const core =
    verdict.core ??
    (['A', 'B', 'C', 'D'] as const).reduce(
      (acc, k) => acc + (t[k]?.mean ?? 0) * (t[k]?.weight ?? 0),
      0
    );
  interface CompComponent {
    key: string;
    label: string;
    value: number;
    weight: number;
  }
  const components: CompComponent[] = [
    { key: 'core', label: 'Core build', value: core, weight: 0.6 },
    { key: 'J', label: 'Journey', value: t.J?.mean, weight: 0.15 },
    { key: 'V', label: 'Visual', value: t.V?.mean, weight: 0.1 },
    { key: 'P', label: 'Performance', value: t.P?.mean, weight: 0.05 },
    { key: 'hard', label: 'Hard block', value: verdict.hard ?? t.HARD?.mean, weight: 0.1 },
  ].filter((c): c is CompComponent => typeof c.value === 'number');

  const width = 860;
  const barH = 26;
  let x = 0;
  const cell = cx('border px-2 py-1', SURFACE.hairline);
  return (
    <div className="overflow-x-auto">
      <div className="min-w-[680px]">
        <svg width={width} height={barH + 20} role="img" aria-label="Score composition">
          {components.map((c) => {
            const slot = c.weight * width;
            const fill = Math.max(2, slot * Math.min(1, Math.max(0, c.value)));
            const g = (
              <g key={c.key}>
                <rect x={x} y={0} width={slot} height={barH} className="fill-lz-surface-2" />
                <rect x={x} y={0} width={fill} height={barH} className="fill-lz-accent" />
                <rect
                  x={x}
                  y={0}
                  width={slot}
                  height={barH}
                  className="fill-none stroke-lz-border"
                />
                <text
                  x={x + slot / 2}
                  y={barH + 14}
                  textAnchor="middle"
                  className={cx('fill-lz-ink-3 text-lz-meta', TNUM)}
                >
                  {pct(c.weight)}
                </text>
              </g>
            );
            x += slot;
            return g;
          })}
        </svg>

        <table className={cx('mt-3 w-full border-collapse text-lz-body text-lz-ink', TNUM)}>
          <thead>
            <tr className="text-left text-lz-zone uppercase text-lz-ink-3">
              <th className={cell}>Component</th>
              <th className={cell}>Earned</th>
              <th className={cell}>Weight</th>
              <th className={cell}>Points of 100</th>
            </tr>
          </thead>
          <tbody>
            {components.map((c) => (
              <tr key={c.key}>
                <td className={cx(cell, WEIGHT.medium)}>{c.label}</td>
                <td className={cell}>{pct(c.value, 1)}</td>
                <td className={cell}>× {pct(c.weight)}</td>
                <td className={cx(cell, WEIGHT.semibold)}>
                  {(c.value * c.weight * 100).toFixed(1)}
                </td>
              </tr>
            ))}
            <tr>
              <td className={cx(cell, WEIGHT.semibold)} colSpan={3}>
                Final score
              </td>
              <td className={cx(cell, WEIGHT.semibold, TONE_TEXT.accent)}>
                {(score * 100).toFixed(1)}
              </td>
            </tr>
          </tbody>
        </table>

        <p className={cx('mt-2 max-w-[80ch]', TYPE.bodyMuted)}>
          Core build = A structure × 25% + B behaviour × 30% + C vendor contract × 25% + D finesse ×
          20%
          {(['A', 'B', 'C', 'D'] as const).every((k) => typeof t[k]?.mean === 'number') && (
            <>
              {' '}
              ={' '}
              {(['A', 'B', 'C', 'D'] as const)
                .map((k) => `${pct(t[k].mean, 0)}·${pct(t[k].weight)}`)
                .join(' + ')}{' '}
              = <span className={cx(WEIGHT.semibold, 'text-lz-ink')}>{pct(core, 1)}</span>
            </>
          )}
          . Six hard checks (idempotency replay, second-sync cost, restart persistence…) are pulled
          out of their home tiers and scored as their own 10% block so tier-mates cannot dilute
          them.
        </p>
      </div>
    </div>
  );
}

/**
 * The isolated payments family's tier weights, read from the verdict itself — never from a table
 * baked here, because the family's weights move between releases: SB7.1 scored S/Q/M as admission
 * gates (weight 0, `admission_only`), SB7.2 weights them into the behavioral score. A tier the
 * scorer weighted is a row with its points; a gate is named as a gate, so a 0% row never reads as
 * a tier that earned nothing.
 */
function PaymentsTierWeights({ tiers }: { tiers: LegacyVerdictDetail['tiers'] }) {
  const recorded = TIER_ORDER.flatMap((tier) => {
    const entry = tiers[tier];
    return entry && typeof entry.mean === 'number' && Number.isFinite(entry.weight)
      ? [{ tier, ...entry }]
      : [];
  });
  const weighted = recorded.filter((row) => !row.admission_only && row.weight > 0);
  const gates = recorded.filter((row) => row.admission_only || row.weight === 0);
  if (weighted.length === 0)
    return <p className={TYPE.bodyMuted}>This result recorded no weighted tiers.</p>;
  const cell = cx('border px-2 py-1', SURFACE.hairline);
  const sum = weighted.reduce((acc, row) => acc + row.mean * row.weight, 0);
  return (
    <section className="overflow-x-auto" aria-label="Weighted tiers">
      <table className={cx('w-full border-collapse text-lz-body text-lz-ink', TNUM)}>
        <thead>
          <tr className="text-left">
            <th className={cell}>Tier</th>
            <th className={cell}>Earned</th>
            <th className={cell}>Weight</th>
            <th className={cell}>Points of 100</th>
          </tr>
        </thead>
        <tbody>
          {weighted.map((row) => (
            <tr key={row.tier}>
              <td className={cell}>
                {row.tier} {TIER_INFO[row.tier]?.name ?? ''}
              </td>
              <td className={cell}>{pct(row.mean, 1)}</td>
              <td className={cell}>{pct(row.weight)}</td>
              <td className={cell}>{(row.mean * row.weight * 100).toFixed(1)}</td>
            </tr>
          ))}
          <tr>
            <td className={cell} colSpan={3}>
              Weighted tier sum
            </td>
            <td className={cx(cell, WEIGHT.semibold)}>{(sum * 100).toFixed(1)}</td>
          </tr>
        </tbody>
      </table>
      {gates.length > 0 && (
        <p className={cx('mt-2', TYPE.bodyMuted)}>
          Admission gates, no weight — they set the ceiling and add no points:{' '}
          {gates
            .map((row) => `${row.tier} ${TIER_INFO[row.tier]?.name ?? ''} ${pct(row.mean, 0)}`)
            .join(' · ')}
          .
        </p>
      )}
    </section>
  );
}

function Sb8Composition({ verdict, score }: { verdict: Sb8VerdictDetail; score: number }) {
  const composition = sb8CompositionSchema(verdict);
  if (
    !composition ||
    typeof verdict.scoreInner !== 'number' ||
    typeof verdict.criticalMultiplier !== 'number'
  ) {
    return (
      <p className={TYPE.bodyMuted}>
        This stored Gauntlet 8 result is missing its composition inputs. The check evidence and tier
        scores remain available.
      </p>
    );
  }
  const { weights, coreTiers } = composition;
  const coreFloor = Math.min(...coreTiers.map((t) => verdict.tiers[t]));
  const excellence = coreFloor * verdict.tiers.E;
  const adjusted = verdict.scoreInner - weights.E * verdict.tiers.E + weights.E * excellence;
  const cell = cx('border px-2 py-1', SURFACE.hairline);
  return (
    <div className="overflow-x-auto">
      {verdict.calibrated === false && <Chip tone="warn">Uncalibrated scorer</Chip>}
      <table className={cx('mt-3 w-full border-collapse text-lz-body text-lz-ink', TNUM)}>
        <thead>
          <tr className="text-left">
            <th className={cell}>Component</th>
            <th className={cell}>Earned</th>
            <th className={cell}>Weight</th>
            <th className={cell}>Points of 100</th>
          </tr>
        </thead>
        <tbody>
          {Object.entries(weights).map(([tier, weight]) => (
            <tr key={tier}>
              <td className={cell}>{SB8_TIERS[tier]?.name ?? tier}</td>
              <td className={cell}>{pct(verdict.tiers[tier], 1)}</td>
              <td className={cell}>{pct(weight)}</td>
              <td className={cell}>{(verdict.tiers[tier] * weight * 100).toFixed(1)}</td>
            </tr>
          ))}
          <tr>
            <td className={cell} colSpan={3}>
              Weighted subtotal
            </td>
            <td className={cell}>{(verdict.scoreInner * 100).toFixed(1)}</td>
          </tr>
          <tr>
            <td className={cell} colSpan={3}>
              Excellence adjustment
            </td>
            <td className={cell}>
              {((excellence - verdict.tiers.E) * weights.E * 100).toFixed(1)}
            </td>
          </tr>
          <tr>
            <td className={cell} colSpan={3}>
              Critical multiplier
            </td>
            <td className={cell}>× {verdict.criticalMultiplier.toFixed(4)}</td>
          </tr>
          <tr>
            <td className={cell} colSpan={3}>
              Final score
            </td>
            <td className={cx(cell, WEIGHT.semibold)}>{(score * 100).toFixed(1)}</td>
          </tr>
        </tbody>
      </table>
      <p className={cx('mt-2 max-w-[80ch]', TYPE.bodyMuted)}>
        Excellence is multiplied by the lowest core tier ({coreTiers.join(', ')}) (
        {pct(coreFloor, 1)}). The adjusted subtotal ({pct(adjusted, 1)}) is multiplied by the
        recorded critical multiplier. Each critical check contributes 0.6 + 0.4 × its score to that
        multiplier.
      </p>
    </div>
  );
}

/** score_forge.py's admission record: the ceiling, its reasons, and every band that failed. */
export interface ForgeAdmissionRecord {
  ceiling: number;
  reasons: string[];
  failedChecksByBand: Array<{ ceiling: number; band: string; checks: string[] }>;
}

const isForgeAdmission = (value: unknown): value is ForgeAdmissionRecord =>
  !!value &&
  typeof value === 'object' &&
  typeof (value as ForgeAdmissionRecord).ceiling === 'number' &&
  Array.isArray((value as ForgeAdmissionRecord).failedChecksByBand);

/** The facts a Forge verdict states about itself before any number: held, unpublishable, rc, shim. */
function ForgeVerdictFacts({
  verdict,
  scorerVersion,
}: {
  verdict: LegacyVerdictDetail;
  scorerVersion?: string;
}) {
  const facts: Array<{ tone: Tone; text: string }> = [];
  if (verdict.status === 'held')
    facts.push({
      tone: 'warn',
      text: `Held for rescore — the emulator met ${verdict.harness_missing?.length ?? 0} call(s) it does not model. Never zeroed, never published as is.`,
    });
  if (verdict.publishable === false)
    facts.push({
      tone: 'err',
      text: `Not publishable: ${(verdict.unpublishable_reasons ?? []).join('; ') || 'the scorer gave no reason'}.`,
    });
  if (verdict.runtime && verdict.runtime !== 'wrapper')
    facts.push({
      tone: 'err',
      text: `Scored on the ${verdict.runtime} runtime, not Atlassian's pinned wrapper.`,
    });
  if (/-rc$/.test(scorerVersion ?? '') || /uncalibrated/i.test(verdict.calibration ?? ''))
    facts.push({
      tone: 'warn',
      text: `${scorerVersion ?? 'This scorer'} is uncalibrated (rc thresholds): a measurement, not a board result.`,
    });
  if (facts.length === 0) return null;
  return (
    <div className="flex flex-col gap-2" data-testid="forge-verdict-facts">
      {facts.map((fact) => (
        <p
          key={fact.text}
          role="status"
          className={cx(
            'px-3 py-2 text-lz-body',
            WEIGHT.medium,
            RADIUS.control,
            TONE_FILL[fact.tone]
          )}
        >
          {fact.text}
        </p>
      ))}
    </div>
  );
}

/**
 * Forge's admission ladder as the verdict recorded it: the earned score, the ceiling the failed bands set,
 * and each failed band with the checks that held it there. The band limits are the scorer's (read from
 * the record, never restated here); passing admission adds no points.
 */
function ForgeAdmission({
  admission,
  rawScore,
  score,
}: {
  admission: unknown;
  rawScore?: number;
  score: number;
}) {
  if (!isForgeAdmission(admission) || typeof rawScore !== 'number')
    return (
      <p role="status" className={TYPE.bodyMuted}>
        This result is missing its admission evidence.
      </p>
    );
  return (
    <section className="flex flex-col gap-3" aria-label="Admission bands">
      <dl className={cx('grid grid-cols-3 gap-3', TNUM)}>
        <div>
          <dt className={TYPE.meta}>Earned before the ceiling</dt>
          <dd className={TYPE.h2}>{rawScore.toFixed(3)}</dd>
        </div>
        <div>
          <dt className={TYPE.meta}>Admission ceiling</dt>
          <dd className={TYPE.h2}>{admission.ceiling.toFixed(3)}</dd>
        </div>
        <div>
          <dt className={TYPE.meta}>Final score</dt>
          <dd className={cx(TYPE.h2, TONE_TEXT.accent)}>{score.toFixed(3)}</dd>
        </div>
      </dl>
      {admission.failedChecksByBand.length === 0 ? (
        <Chip tone="ok" icon={<Check />}>
          Every admission band passed — no ceiling
        </Chip>
      ) : (
        admission.failedChecksByBand.map((band) => (
          <div
            key={band.band}
            data-testid="forge-failed-band"
            className={cx('flex flex-col gap-2', SURFACE.card, SPACE.card)}
          >
            <div className="flex flex-wrap items-center gap-2">
              <Chip tone="err" icon={<X />}>
                Capped at {band.ceiling.toFixed(3)}
              </Chip>
              <span className={cx(TYPE.body, WEIGHT.semibold)}>{band.band}</span>
            </div>
            <div className="flex flex-wrap gap-1">
              {band.checks.map((check) => (
                <Chip key={check}>{check}</Chip>
              ))}
            </div>
          </div>
        ))
      )}
      <p className={TYPE.bodyMuted}>
        The final score is the lower of earned credit and the ceiling of the lowest band a required
        check failed. Passing a band adds no points.
      </p>
    </section>
  );
}

/** Forge's weighted tiers (L…A) and the E slice, from the verdict — weights read, never restated. */
function ForgeComposition({ verdict }: { verdict: LegacyVerdictDetail }) {
  const rows = FORGE_TIER_ORDER.flatMap((tier) => {
    const entry = verdict.tiers[tier];
    return entry && typeof entry.mean === 'number' && Number.isFinite(entry.weight)
      ? [{ tier, ...entry }]
      : [];
  });
  const cell = cx('border px-2 py-1', SURFACE.hairline);
  const critical = verdict.critical;
  return (
    <section className="flex flex-col gap-2" aria-label="Earned score composition">
      <div className="overflow-x-auto">
        <table className={cx('w-full border-collapse text-lz-body text-lz-ink', TNUM)}>
          <thead>
            <tr className="text-left">
              <th className={cell}>Tier</th>
              <th className={cell}>Earned</th>
              <th className={cell}>Weight</th>
              <th className={cell}>Points of 100</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.tier}>
                <td className={cell}>
                  {row.tier} {FORGE_TIERS[row.tier]?.name ?? ''}
                </td>
                <td className={cell}>{pct(row.mean, 1)}</td>
                <td className={cell}>{pct(row.weight)}</td>
                <td className={cell}>{(row.mean * row.weight * 100).toFixed(1)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {typeof verdict.inner === 'number' && critical && verdict.excellence ? (
        <p className={TYPE.body}>
          Recorded composition inputs: inner score {verdict.inner.toFixed(4)} · excellence admission{' '}
          {verdict.excellence.fraction.toFixed(4)} · excellence mean{' '}
          {verdict.excellence.e_mean.toFixed(4)}
          {typeof critical.pre_severity_score === 'number'
            ? ` · before criticals ${critical.pre_severity_score.toFixed(4)}`
            : ''}{' '}
          · critical multiplier {critical.multiplier.toFixed(4)}.
        </p>
      ) : (
        <p className={TYPE.bodyMuted}>Earned-score composition evidence is unavailable.</p>
      )}
      {(critical?.rows ?? []).map((row) => (
        <p key={row.check} className={TYPE.bodyMuted}>
          {row.check}: factor {row.factor ?? 'unavailable'}
          {row.suppressed ? ` (suppressed — ${row.suppressed})` : ''} · {row.why}
        </p>
      ))}
    </section>
  );
}

function RepairStrip({ rounds }: { rounds: Array<{ round: number; findings: number }> }) {
  if (rounds.length === 0) return null;
  return (
    <div>
      <div className="flex flex-wrap items-center gap-2">
        {rounds.map((r, i) => (
          <span key={r.round} className="flex items-center gap-2">
            {i > 0 && <span className="text-lz-body text-lz-ink-3">→</span>}
            <Chip tone={r.findings === 0 ? 'ok' : 'err'}>
              Round {r.round} · {r.findings} finding{r.findings === 1 ? '' : 's'}
            </Chip>
          </span>
        ))}
      </div>
      <p className={cx('mt-2 max-w-[80ch]', TYPE.bodyMuted)}>
        At each round the completion gate runs the app, opens the page in a real browser, and files
        findings; the swarm's repair waves fix them and the gate re-verifies — until it reads clean,
        the count stops moving, or the round budget is spent.
      </p>
    </div>
  );
}

export function ScoringDetail({
  verdict: rawVerdict,
  score,
  scorerVersion,
}: {
  verdict: VerdictDetail;
  score: number;
  scorerVersion?: string;
}) {
  const sb8 = isSb8(
    scorerVersion ?? ('scorerVersion' in rawVerdict ? rawVerdict.scorerVersion : '')
  );
  const sb8Verdict = sb8 ? (rawVerdict as Sb8VerdictDetail) : null;
  const paymentsTier =
    isolatedPaymentsTier(scorerVersion) ??
    isolatedPaymentsTier('scorerVersion' in rawVerdict ? rawVerdict.scorerVersion : undefined);
  const forge = isForge(
    scorerVersion ?? ('scorerVersion' in rawVerdict ? rawVerdict.scorerVersion : undefined)
  );
  const verdict: LegacyVerdictDetail = useMemo(
    () =>
      sb8Verdict
        ? {
            checks: sb8Verdict.checks.map((c) => ({ ...c, check: c.name })),
            tiers: Object.fromEntries(
              Object.entries(sb8Verdict.tiers).map(([tier, mean]) => [
                tier,
                {
                  mean,
                  checks: sb8Verdict.checks.filter((c) => c.tier === tier).length,
                  weight: sb8CompositionSchema(sb8Verdict)?.weights[tier] ?? NaN,
                },
              ])
            ),
          }
        : (rawVerdict as LegacyVerdictDetail),
    [rawVerdict, sb8Verdict]
  );
  const groups = useMemo(() => {
    const byTier = new Map<string, VerdictCheck[]>();
    for (const c of verdict.checks) {
      const list = byTier.get(c.tier) ?? [];
      list.push(c);
      byTier.set(c.tier, list);
    }
    const order: readonly string[] = forge ? FORGE_TIER_ORDER : TIER_ORDER;
    return order
      .filter((t) => (byTier.get(t) ?? []).length > 0)
      .map((t) => ({
        tier: t as string,
        checks: byTier.get(t) ?? [],
        mean: typeof verdict.tiers[t]?.mean === 'number' ? verdict.tiers[t].mean : null,
        weight: Number.isFinite(verdict.tiers[t]?.weight) ? verdict.tiers[t].weight : null,
        admissionOnly: verdict.tiers[t]?.admission_only === true,
      }));
  }, [verdict, forge]);

  // Open the WORST imperfect tier by default — the click the user was going to make anyway.
  const [open, setOpen] = useState<Record<string, boolean>>(() => {
    const imperfect = groups.filter((g) => g.mean != null && g.mean < 1);
    if (imperfect.length === 0) return {};
    const worst = imperfect.reduce((a, b) => ((b.mean ?? 1) < (a.mean ?? 1) ? b : a));
    return { [worst.tier]: true };
  });

  const findingsHeld = verdict.findingsHeld ?? [];
  const rootCauses = Object.entries(verdict.root_causes ?? {});

  return (
    <div className="flex flex-col gap-6">
      <>
        {sb8Verdict ? (
          <Sb8Composition verdict={sb8Verdict} score={score} />
        ) : forge ? (
          <>
            <ForgeVerdictFacts verdict={verdict} scorerVersion={scorerVersion} />
            <ForgeAdmission
              admission={verdict.admission}
              rawScore={verdict.rawScore}
              score={score}
            />
            <ForgeComposition verdict={verdict} />
          </>
        ) : paymentsTier ? (
          <>
            <ScoreAdmission
              admission={verdict.admission as Admission | undefined}
              rawScore={verdict.rawScore}
              score={score}
            />
            <PaymentsTierWeights tiers={verdict.tiers} />
            {typeof verdict.inner === 'number' && verdict.critical && verdict.excellence ? (
              <section className="flex flex-col gap-2" aria-label="Earned score composition">
                {paymentsTier === 'sb-7.1' ? (
                  <p className={TYPE.body}>
                    Earned credit: (0.88 × behavioral score {verdict.inner.toFixed(4)} + 0.12 ×
                    excellence admission {verdict.excellence.fraction.toFixed(4)} × excellence mean{' '}
                    {verdict.excellence.e_mean.toFixed(4)}) × critical multiplier{' '}
                    {verdict.critical.multiplier.toFixed(4)}.
                  </p>
                ) : (
                  // Later payments tiers record their inputs, not their constants: the numbers
                  // below are the scorer's own, and no formula from an earlier release is
                  // re-stated around them.
                  <p className={TYPE.body}>
                    Recorded composition inputs: behavioral score {verdict.inner.toFixed(4)} ·
                    excellence admission {verdict.excellence.fraction.toFixed(4)} · excellence mean{' '}
                    {verdict.excellence.e_mean.toFixed(4)} · critical multiplier{' '}
                    {verdict.critical.multiplier.toFixed(4)}.
                  </p>
                )}
                {verdict.critical.rows.map((row) => (
                  <p key={row.check} className={TYPE.bodyMuted}>
                    {row.check}: factor {row.factor ?? 'unavailable'} · {row.why}
                  </p>
                ))}
                <details>
                  <summary className={TYPE.body}>Excellence evidence</summary>
                  <pre className="whitespace-pre-wrap break-words">
                    {JSON.stringify(verdict.excellence.conditions, null, 2)}
                  </pre>
                </details>
              </section>
            ) : (
              <p className={TYPE.bodyMuted}>Earned-score composition evidence is unavailable.</p>
            )}
            {(['probe_unavailable', 'vacuous', 'harness_missing', 'sched_unreached'] as const).map(
              (key) =>
                verdict[key]?.length ? (
                  <p key={key} role="status" className={TYPE.body}>
                    {key.replace(/_/g, ' ')}: {JSON.stringify(verdict[key])}
                  </p>
                ) : null
            )}
          </>
        ) : (
          <CompositionBar verdict={verdict} score={score} />
        )}
      </>

      {findingsHeld.length > 0 && (
        // The refusal register: a solid err header on a Panel-shaped card; the findings themselves
        // stay in ink on the surface so a long verbatim line is still readable.
        <section data-testid="findings-held" className={cx(SURFACE.card, 'overflow-hidden')}>
          <div
            className={cx(
              'flex min-h-10 items-center gap-2 px-4 py-2 [&>svg]:size-4 [&>svg]:shrink-0',
              TONE_FILL.err
            )}
          >
            <XCircle />
            <span className="text-lz-zone uppercase">
              Findings that held — the gate still saw these when verification ended
            </span>
          </div>
          <div className={cx('flex flex-col gap-2', SPACE.card)}>
            {findingsHeld.map((f, i) => (
              <p
                key={`${i}:${f}`}
                className="whitespace-pre-wrap break-words font-mono text-lz-mono text-lz-ink"
              >
                {f}
              </p>
            ))}
          </div>
        </section>
      )}

      {(verdict.repairRounds ?? []).length > 0 && (
        <div>
          <SectionHeader as="h3" title="Repair progression" className="mb-2" />
          <RepairStrip rounds={verdict.repairRounds ?? []} />
        </div>
      )}

      {rootCauses.length > 0 && (
        <section className={cx(SURFACE.card, SPACE.card)}>
          <SectionHeader as="h3" title="Root-cause attribution" />
          {rootCauses.map(([root, downstream]) => (
            <p key={root} className={cx('mt-2', TYPE.body)}>
              <span className={cx(WEIGHT.semibold, TONE_TEXT.err)}>{humanize(root)}</span> failed at
              the root and zeroed {downstream.length} downstream check
              {downstream.length === 1 ? '' : 's'}: {downstream.map(humanize).join(', ')} — one
              defect, not {downstream.length + 1}.
            </p>
          ))}
        </section>
      )}

      <div className="flex flex-col gap-3">
        {groups.map((g) => (
          <TierGroup
            key={g.tier}
            tier={g.tier}
            sb8={sb8}
            forge={forge}
            checks={g.checks}
            mean={g.mean}
            weight={g.weight}
            admissionOnly={g.admissionOnly}
            open={!!open[g.tier]}
            onToggle={() => setOpen((o) => ({ ...o, [g.tier]: !o[g.tier] }))}
          />
        ))}
      </div>
    </div>
  );
}
