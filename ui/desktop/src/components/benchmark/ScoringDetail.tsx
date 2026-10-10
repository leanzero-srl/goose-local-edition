import { ScoreAdmission, type Admission } from './ScoreAdmission';
import { useMemo, useState } from 'react';
import {
  FORGE_TIERS,
  FORGE_TIER_ORDER,
  SB8_TIERS,
  VERDICT_TIER_INFO as TIER_INFO,
  VERDICT_TIER_ORDER as TIER_ORDER,
  eraDisplayName,
  isForge,
  isSb8,
  isolatedPaymentsTier,
  scoresAsDecimals,
  weightText,
} from './baselines';
import { sb8CompositionSchema } from '../../sb8ScoreSchema';
import { forgeHeldWords, forgeUnpublishableWords } from '../../benchForgePublish';
import { forgeHasReliability, readForgeReliability } from '../../benchForgeReliability';
import { summarizeParts, type PartLeaf } from './partSummary';
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
    rows: Array<{
      check: string;
      /** score_forge2.py: the kind of critical defect the test observed (`duplicate`, `leak`, …). */
      class?: string;
      factor?: number;
      why?: string;
      suppressed?: string;
    }>;
  };
  excellence?: { fraction: number; e_mean: number; conditions: unknown };
  /** Forge 2.0 (score_forge2.py): the failed tests that multiply the score — read through
   *  readForgeReliability, never trusted as typed. */
  reliability?: unknown;
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

/** A group's mean or a test's score. Forge 2.0 prints every score as leanzero.net does — four decimals — so
 *  the chip on a test reads the same digits as the reliability line that names it; the other eras keep 0–100. */
function ScoreChip({ score, decimals = false }: { score: number; decimals?: boolean }) {
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
      {decimals ? score.toFixed(4) : Math.round(score * 100)}
    </span>
  );
}

/** parts — the scorer's per-item evidence map. Booleans as solid check/cross chips, numbers inline,
 *  each nested object as its own row of key: value chips (partSummary.ts — never "[object Object]"). */
function PartLeafChip({ leaf }: { leaf: PartLeaf }) {
  if (leaf.kind === 'flag')
    return (
      <Chip tone={leaf.ok ? 'ok' : 'err'} icon={leaf.ok ? <Check /> : <X />}>
        {leaf.key}
      </Chip>
    );
  return (
    <Chip tone={leaf.tone}>
      {leaf.key} {leaf.text}
    </Chip>
  );
}

export function PartChips({ parts }: { parts: Record<string, unknown> }) {
  const views = summarizeParts(Object.fromEntries(Object.entries(parts).slice(0, 24)));
  if (views.length === 0) return null;
  const leaves = views.filter((v): v is PartLeaf => v.kind !== 'group');
  const groups = views.filter((v) => v.kind === 'group');
  return (
    <div className="mt-1.5 flex flex-col gap-1.5">
      {leaves.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {leaves.map((leaf) => (
            <PartLeafChip key={leaf.key} leaf={leaf} />
          ))}
        </div>
      )}
      {groups.map((group) => (
        <div
          key={group.key}
          data-testid="part-group"
          data-part={group.key}
          className="flex flex-wrap items-center gap-1"
        >
          {group.ok == null ? (
            <span className={cx('mr-0.5 font-mono text-lz-mono text-lz-ink-2', WEIGHT.semibold)}>
              {group.key}
            </span>
          ) : (
            <Chip tone={group.ok ? 'ok' : 'err'} icon={group.ok ? <Check /> : <X />}>
              {group.key}
            </Chip>
          )}
          {group.items.map((leaf) => (
            <PartLeafChip key={leaf.key} leaf={leaf} />
          ))}
          {group.more > 0 && <Chip>+{group.more} more</Chip>}
        </div>
      ))}
    </div>
  );
}

function CheckRow({ check, decimals }: { check: VerdictCheck; decimals: boolean }) {
  return (
    <div className={cx('flex items-start gap-3 border-t px-3 py-2.5', SURFACE.hairline)}>
      <ScoreChip score={check.score} decimals={decimals} />
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
  decimals = false,
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
  /** Scores print as four decimals (Forge 2.0), not 0–100. */
  decimals?: boolean;
}) {
  const info = (sb8 ? SB8_TIERS[tier] : forge ? FORGE_TIERS[tier] : TIER_INFO[tier]) ?? {
    name: tier,
    desc: '',
  };
  // Forge surfaces say "test", the word of its task text and of leanzero.net; the Gauntlet eras keep "check".
  const unit = (count: number) => (forge ? 'test' : 'check') + (count === 1 ? '' : 's');
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
          // A count of tests, never of points: three tests at 0.99 are three, and cost far less than three.
          <Chip tone="err">
            {lost} {unit(lost)} below full marks
          </Chip>
        )}
        <span className={cx('shrink-0', TYPE.meta, TNUM)}>
          {checks.length} {forge ? unit(checks.length) : 'checks'}
          {admissionOnly
            ? ' · admission gate'
            : weight != null
              ? ` · weight ${weightText(weight)}`
              : ''}
        </span>
        {mean != null && <ScoreChip score={mean} decimals={decimals} />}
      </button>
      {open && (
        <div>
          {checks.map((c) => (
            <CheckRow key={c.check} check={c} decimals={decimals} />
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

/** score_forge*.py's cap record: the lowest cap that applies, its reasons, every cap whose tests failed and —
 *  since the scorers pull a capped final below its cap — the rule the final score was computed by. */
export interface ForgeAdmissionRecord {
  ceiling: number;
  reasons: string[];
  failedChecksByBand: Array<{ ceiling: number; band: string; checks: string[] }>;
  final_rule?: string;
}

const isForgeAdmission = (value: unknown): value is ForgeAdmissionRecord =>
  !!value &&
  typeof value === 'object' &&
  typeof (value as ForgeAdmissionRecord).ceiling === 'number' &&
  Array.isArray((value as ForgeAdmissionRecord).failedChecksByBand);

/**
 * Forge 2.0's caps and critical defects in the words of its task text's Score section (the text the model
 * receives, and the words leanzero.net prints), keyed by the scorer's own label and class. A label or class
 * the task text does not word is shown as the scorer recorded it. The scorer's own string for `duplicate`
 * leaves out the replayed CI event, although r6_replay_once is charged to that class.
 */
const FORGE2_CAP_WORDS: Record<string, string> = {
  deployable: 'Lint errors, or manifest functions that do not bundle and load.',
  'v2 surfaces':
    'None of the v2 surfaces exists: no jira:adminPage, no webtrigger and no scope-ledger entity.',
};
const FORGE2_CRITICAL_WORDS: Record<string, string> = {
  migration: 'v1 rows lost or corrupted by the migration',
  webtrigger: 'a web-trigger write without a valid signature',
  admin: 'an admin action by a non-admin that succeeded, or the CI secret disclosed',
  leak: "a hidden issue's data shown to a person who cannot browse it",
  duplicate:
    'a duplicate side effect: two or more comments for one click, duplicate ledger rows, or a replayed CI event applied twice',
};

/** The facts a Forge verdict states about itself before any number: on hold, unpublishable, scored before
 *  its thresholds were final, another runtime — in plain words, the same ones the publish refusal uses. */
function ForgeVerdictFacts({
  verdict,
  scorerVersion,
}: {
  verdict: LegacyVerdictDetail;
  scorerVersion?: string;
}) {
  const facts: Array<{ tone: Tone; text: string }> = [];
  const held = verdict.status === 'held';
  if (held) facts.push({ tone: 'warn', text: forgeHeldWords(verdict).sentence });
  // A held verdict is unpublishable BECAUSE it is held: the scorer lists no further reason, and "no reason
  // recorded" under the hold would contradict the line above it.
  if (verdict.publishable === false && !(held && !verdict.unpublishable_reasons?.length))
    facts.push({
      tone: 'err',
      text: `Cannot be published: ${forgeUnpublishableWords(verdict)
        .map(({ what }) => what)
        .join(' Also: ')}`,
    });
  // The scorer lists another runtime among its unpublishable reasons; a verdict that names one without
  // that list still says so.
  else if (verdict.runtime && verdict.runtime !== 'wrapper')
    facts.push({
      tone: 'err',
      text: `Scored on the ${verdict.runtime} runtime, not Atlassian's pinned Forge runtime.`,
    });
  if (/-rc$/.test(scorerVersion ?? '') || /uncalibrated/i.test(verdict.calibration ?? '')) {
    const era = scorerVersion ? `${eraDisplayName(scorerVersion.replace(/-rc$/, ''))}'s` : 'the';
    facts.push({
      tone: 'warn',
      text: `Scored before ${era} thresholds were final: a measurement, not a board result.`,
    });
  }
  // A Forge 2.0 verdict kept from before the reliability rule: its number has no such step, and saying
  // nothing would read as "no test failed".
  if (forgeHasReliability(scorerVersion) && !readForgeReliability(verdict.reliability))
    facts.push({
      tone: 'warn',
      text: 'Scored before failed tests multiplied the score: this result carries no reliability record, so its number leaves that step out. Re-score the saved build to see it under the current rule.',
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

/** The pull a capped final keeps below its cap, as the verdict's own rule states it
 *  (`final = min(earned, ceiling - 0.05 * (1 - earned))` → "0.05") — read, never typed here. */
const capPull = (rule: string | undefined) =>
  /^final = min\(earned, ceiling - ([0-9.]+) \* \(1 - earned\)\)$/.exec(rule ?? '')?.[1] ?? null;

/**
 * Forge's caps and final score as the verdict recorded them: the earned score, the lowest cap that applies,
 * the final score, each cap with the tests that set it, and the rule that takes the first two to the third.
 *
 * THE RULE IS THE VERDICT'S, PER VERDICT. Since 2026-10-03 both Forge scorers pull a capped final below its
 * cap (capped_final: min(earned, cap − pull × (1 − earned))) and write `admission.final_rule`; a verdict
 * scored before that has no such field and its final is the lower of the earned score and the cap. Stating
 * the older rule under a pulled final contradicted the three numbers printed above it (earned 0.7000, cap
 * 0.499, final 0.4840: neither), so the sentence follows the field. The worked line prints the recorded
 * numbers only, and only when the rule applied to them gives the recorded final to the digit shown — the
 * arithmetic is checked here, never used as the score.
 */
function ForgeAdmission({
  admission,
  rawScore,
  score,
  forge2,
}: {
  admission: unknown;
  rawScore?: number;
  score: number;
  forge2: boolean;
}) {
  if (!isForgeAdmission(admission) || typeof rawScore !== 'number')
    return (
      <p role="status" className={TYPE.bodyMuted}>
        This result is missing its record of score caps.
      </p>
    );
  // Forge 2.0 prints a score to four decimals everywhere; Forge 1.0's history keeps its three here.
  const digits = forge2 ? 4 : 3;
  const capped = admission.ceiling < 1;
  const pull = capPull(admission.final_rule);
  const worked =
    capped &&
    pull != null &&
    Math.min(rawScore, admission.ceiling - Number(pull) * (1 - rawScore)).toFixed(4) ===
      score.toFixed(4)
      ? `Here: min(${rawScore.toFixed(4)}, ${admission.ceiling.toFixed(3)} − ${pull} × (1 − ${rawScore.toFixed(4)})) = ${score.toFixed(4)}.`
      : null;
  return (
    <section className="flex flex-col gap-3" aria-label="Final score">
      {forge2 && <SectionHeader as="h3" title="Final score" />}
      <dl className={cx('grid grid-cols-3 gap-3', TNUM)}>
        <div>
          <dt className={TYPE.meta}>Earned score</dt>
          <dd className={TYPE.h2}>{rawScore.toFixed(digits)}</dd>
        </div>
        <div>
          <dt className={TYPE.meta}>Cap</dt>
          <dd className={TYPE.h2}>{capped ? admission.ceiling.toFixed(3) : 'none'}</dd>
        </div>
        <div>
          <dt className={TYPE.meta}>Final score</dt>
          <dd className={cx(TYPE.h2, TONE_TEXT.accent)}>{score.toFixed(digits)}</dd>
        </div>
      </dl>
      {admission.failedChecksByBand.length === 0 ? (
        <div>
          <Chip tone="ok" icon={<Check />}>
            No cap applies
          </Chip>
        </div>
      ) : (
        admission.failedChecksByBand.map((band) => (
          <div
            key={band.band}
            data-testid="forge-failed-band"
            className={cx('flex flex-col gap-2', SURFACE.card, SPACE.card)}
          >
            <div className="flex flex-wrap items-center gap-2">
              <Chip tone="err" icon={<X />}>
                Maximum {band.ceiling.toFixed(3)}
              </Chip>
              <span className={cx(TYPE.body, WEIGHT.semibold)}>
                {(forge2 ? FORGE2_CAP_WORDS[band.band] : undefined) ?? band.band}
              </span>
            </div>
            <div className="flex flex-wrap items-center gap-1">
              <span className={TYPE.meta}>
                Failed {band.checks.length === 1 ? 'test' : 'tests'}:
              </span>
              {band.checks.map((check) => (
                <Chip key={check}>{humanize(check)}</Chip>
              ))}
            </div>
          </div>
        ))
      )}
      <p data-testid="forge-cap-rule" className={cx('max-w-[80ch]', TYPE.bodyMuted)}>
        {pull != null
          ? `Under a cap the final score is min(earned, cap − ${pull} × (1 − earned)): below the cap, and closer to it the more the run earned. When more than one cap applies, the lowest counts. With no cap the final score is the earned score. Avoiding a cap adds no points.`
          : admission.final_rule
            ? `The scorer recorded its rule for the final score as: ${admission.final_rule}. Avoiding a cap adds no points.`
            : 'The final score is the lower of the earned score and the lowest cap that applies. Avoiding a cap adds no points.'}
        {worked ? ` ${worked}` : ''}
      </p>
    </section>
  );
}

/** What a Forge 2.0 verdict records of its steps, or null when it carries no reliability record (a verdict
 *  scored before the rule: named in ForgeVerdictFacts, and no step it never recorded is drawn). */
function forgeSteps(verdict: LegacyVerdictDetail, scorerVersion: string | undefined) {
  const reliability = forgeHasReliability(scorerVersion)
    ? readForgeReliability(verdict.reliability)
    : null;
  const tests = verdict.critical?.pre_severity_score;
  const critical = verdict.critical?.multiplier;
  const earned = verdict.rawScore;
  return reliability &&
    typeof tests === 'number' &&
    typeof critical === 'number' &&
    typeof earned === 'number'
    ? { reliability, tests, critical, earned }
    : null;
}

/**
 * Forge 2.0's score in one line, in the order and by the names every Forge surface uses: Tests earned ×
 * Critical defects × Reliability = Final score. The sections under it explain each step in the same order.
 * Every number is the verdict's (`critical.pre_severity_score`, `critical.multiplier`,
 * `reliability.multiplier`, `rawScore`); nothing is multiplied here. Under a cap the product is the earned
 * score, and the Final score section takes it from there.
 */
function ForgeScoreSteps({
  verdict,
  score,
  scorerVersion,
}: {
  verdict: LegacyVerdictDetail;
  score: number;
  scorerVersion?: string;
}) {
  const recorded = forgeSteps(verdict, scorerVersion);
  if (!recorded) return null;
  const { reliability, tests, critical, earned } = recorded;
  const isFinal = Math.abs(score - earned) < 5e-5;
  const steps = [
    { key: 'tests', label: 'Tests earned', value: tests },
    { key: 'critical', label: '× Critical defects', value: critical },
    { key: 'reliability', label: '× Reliability', value: reliability.multiplier },
    {
      key: 'earned',
      label: isFinal ? '= Final score' : '= Earned score, before the cap',
      value: earned,
    },
  ];
  return (
    <section aria-label="Score steps">
      <dl className={cx('grid grid-cols-2 gap-3 sm:grid-cols-4', TNUM)}>
        {steps.map((step) => (
          <div key={step.key} data-testid="forge-step" data-step={step.key}>
            <dt className={TYPE.meta}>{step.label}</dt>
            <dd
              className={cx(
                'flex flex-wrap items-center gap-2',
                TYPE.h2,
                step.key === 'earned' && isFinal && TONE_TEXT.accent
              )}
            >
              {step.value.toFixed(4)}
              {step.key === 'reliability' && reliability.floored && (
                <Chip tone="warn">at the floor</Chip>
              )}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

/**
 * Step one, Forge 2.0: what the tests earned — every group of tests with the mean of its tests, its weight
 * and what the two add to the score, closing on the verdict's own "Tests earned" (`critical.pre_severity_score`,
 * the number the steps start from). Weights are the verdict's, printed at the precision that adds to 100%.
 * The lines are on the score's own scale and to its four decimals, so the table and the step above it state
 * one number one way; the total is the recorded one, never the sum of the rounded lines.
 */
function ForgeTestsEarned({ verdict }: { verdict: LegacyVerdictDetail }) {
  const rows = FORGE_TIER_ORDER.flatMap((tier) => {
    const entry = verdict.tiers[tier];
    return entry && typeof entry.mean === 'number' && Number.isFinite(entry.weight)
      ? [{ tier, ...entry }]
      : [];
  });
  const cell = cx('border px-2 py-1', SURFACE.hairline);
  const tests = verdict.critical?.pre_severity_score;
  const excellence = verdict.excellence;
  const conditions = Array.isArray(excellence?.conditions) ? excellence.conditions.length : null;
  return (
    <section className="flex flex-col gap-2" aria-label="Tests earned">
      <SectionHeader as="h3" title="Tests earned" />
      <p className={cx('max-w-[80ch]', TYPE.bodyMuted)}>
        Tests earn the score. Each group of tests adds the mean of its tests × its weight; added up,
        the groups are what the tests earned.
      </p>
      <div className="overflow-x-auto">
        <table className={cx('w-full border-collapse text-lz-body text-lz-ink', TNUM)}>
          <thead>
            <tr className="text-left">
              <th className={cell}>Group</th>
              <th className={cell}>Mean of its tests</th>
              <th className={cell}>Weight</th>
              <th className={cell}>Adds to the score</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.tier} data-testid="forge-group-line" data-tier={row.tier}>
                <td className={cell}>
                  {row.tier} {FORGE_TIERS[row.tier]?.name ?? ''}
                </td>
                <td className={cell}>{row.mean.toFixed(4)}</td>
                <td className={cell}>{weightText(row.weight)}</td>
                <td className={cell}>{(row.mean * row.weight).toFixed(4)}</td>
              </tr>
            ))}
            {typeof tests === 'number' && (
              <tr data-testid="forge-tests-earned">
                <td className={cx(cell, WEIGHT.semibold)} colSpan={2}>
                  Tests earned
                </td>
                <td className={cx(cell, WEIGHT.semibold)}>
                  {weightText(rows.reduce((sum, row) => sum + row.weight, 0))}
                </td>
                <td className={cx(cell, WEIGHT.semibold)}>{tests.toFixed(4)}</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
      {typeof tests === 'number' ? (
        <p className={cx('max-w-[80ch]', TYPE.bodyMuted)}>
          {excellence
            ? `E ${FORGE_TIERS.E.name} is the excellence gate ${excellence.fraction.toFixed(4)}${conditions ? ` (the average of its ${conditions} conditions)` : ''} × the mean of its tests ${excellence.e_mean.toFixed(4)}. `
            : ''}
          Each line is rounded to four decimals, so added up they can differ from the total in the
          last digit.
        </p>
      ) : (
        <p role="status" className={TYPE.bodyMuted}>
          This result did not record what its tests earned.
        </p>
      )}
    </section>
  );
}

type ForgeCriticalRow = NonNullable<LegacyVerdictDetail['critical']>['rows'][number];

/** One critical-defect line in plain words: the test that observed the defect and what it cost, or why it
 *  cost nothing more. The scorer's notes (`class:duplicate priced by t_no_double_count`, `root:<test>`) are
 *  read, never printed; a note in a form this view does not know is quoted as recorded. */
function criticalLine(row: ForgeCriticalRow): string {
  const name = humanize(row.check);
  const words = (row.class ? FORGE2_CRITICAL_WORDS[row.class] : undefined) ?? row.why;
  const sameKind = /^class:\S+ priced by (\S+)$/.exec(row.suppressed ?? '');
  if (sameKind)
    return `${name}: no additional penalty. ${humanize(sameKind[1])} already multiplied the score for this kind of defect, and each kind multiplies it once.`;
  const sameDefect = /^root:(\S+)$/.exec(row.suppressed ?? '');
  if (sameDefect)
    return `${name}: no additional penalty. The scorer counts it as the same defect as ${humanize(sameDefect[1])}, which already multiplied the score.`;
  if (row.suppressed)
    return `${name}: no additional penalty (the scorer's note: ${row.suppressed}).`;
  if (typeof row.factor !== 'number')
    return `${name}: the scorer recorded no factor${words ? ` (${words})` : ''}.`;
  return `${name} × ${row.factor.toFixed(2)}${words ? `: ${words}` : ''}.`;
}

/** Step two, Forge 2.0: the critical defects — the rule with the verdict's own factor, and one line per test
 *  that observed one. */
function ForgeCriticalDefects({ verdict }: { verdict: LegacyVerdictDetail }) {
  const critical = verdict.critical;
  if (!critical) return null;
  const rows = critical.rows ?? [];
  const factor = typeof critical.floor === 'number' ? ` by ${critical.floor.toFixed(2)}` : '';
  return (
    <section className="flex flex-col gap-2" aria-label="Critical defects">
      <SectionHeader as="h3" title="× Critical defects" />
      <p className={cx('max-w-[80ch]', TYPE.bodyMuted)}>
        Each kind of critical defect that is observed multiplies the whole score{factor}, at most
        once however many tests observe it.
      </p>
      {rows.length === 0 && critical.multiplier === 1 && (
        <div>
          <Chip tone="ok" icon={<Check />}>
            No critical defect was observed
          </Chip>
        </div>
      )}
      {rows.map((row) => (
        <p key={row.check} data-testid="critical-line" className={TYPE.body}>
          {criticalLine(row)}
        </p>
      ))}
    </section>
  );
}

/**
 * Step three, Forge 2.0: reliability — the rule in the task text's one sentence (its two numbers read from the
 * verdict) and the groups of tests that make up the factor: one line per group that multiplies the score (its
 * worst test, that test's score, the group's factor), the group's other failed tests under it as already
 * counted, and the tests a critical defect priced instead. Nothing is multiplied here, so the lines read as
 * the scorer wrote them. The floor is on the groups' product only: a critical defect multiplies on top of it.
 */
function ForgeReliability({
  verdict,
  scorerVersion,
}: {
  verdict: LegacyVerdictDetail;
  scorerVersion?: string;
}) {
  const reliability = forgeSteps(verdict, scorerVersion)?.reliability;
  if (!reliability) return null;
  const cell = cx('border px-2 py-1 align-top', SURFACE.hairline);
  const floor = reliability.floor.toFixed(2);
  const groupName = (tier: string) => `${tier} ${FORGE_TIERS[tier]?.name ?? ''}`.trim();
  // A group's other failed tests sit under its line; a group the scorer listed there with no line of its own
  // is still shown, never dropped.
  const lined = new Set(reliability.defects.map((defect) => defect.tier));
  const unlined = Object.entries(reliability.folded).filter(
    ([tier, names]) => !lined.has(tier) && names.length > 0
  );
  return (
    <section className="flex flex-col gap-3" aria-label="Reliability">
      <SectionHeader as="h3" title="× Reliability" />
      <p data-testid="reliability-rule" className={cx('max-w-[80ch]', TYPE.bodyMuted)}>
        Each group of tests multiplies the score by 1 − {reliability.k.toFixed(2)} × the shortfall
        of its worst test: by {(1 - reliability.k).toFixed(2)} when that test fails completely, by 1
        when every test in the group passes. The group&rsquo;s other failed tests are already
        counted by its worst one. E {FORGE_TIERS.E.name} never multiplies. Together the groups never
        take the score below {floor} of what the tests earned; a critical defect still multiplies on
        top.
      </p>
      {reliability.defects.length === 0 ? (
        <div>
          <Chip tone="ok" icon={<Check />}>
            No group of tests multiplied the score
          </Chip>
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table
            aria-label="Groups of tests that multiply the score"
            className={cx('w-full border-collapse text-lz-body text-lz-ink', TNUM)}
          >
            <thead>
              <tr className="text-left">
                <th className={cell}>Group</th>
                <th className={cell}>Its worst test</th>
                <th className={cell}>That test&rsquo;s score</th>
                <th className={cell}>Multiplies the score by</th>
              </tr>
            </thead>
            <tbody>
              {reliability.defects.map((defect) => {
                const others = reliability.folded[defect.tier] ?? [];
                return (
                  <tr key={defect.tier} data-testid="reliability-line" data-tier={defect.tier}>
                    <td className={cx(cell, WEIGHT.medium)}>{groupName(defect.tier)}</td>
                    <td className={cell}>
                      {humanize(defect.check)}
                      {others.length > 0 && (
                        <div data-testid="reliability-folded" className={cx('mt-0.5', TYPE.meta)}>
                          Already counted in this group: {others.map(humanize).join(', ')}
                        </div>
                      )}
                    </td>
                    <td className={cell}>{defect.score.toFixed(4)}</td>
                    <td className={cell}>× {defect.factor.toFixed(4)}</td>
                  </tr>
                );
              })}
              <tr data-testid="reliability-total">
                <td className={cx(cell, WEIGHT.semibold)} colSpan={3}>
                  Reliability{reliability.floored ? ' — at the floor' : ''}
                </td>
                <td className={cx(cell, WEIGHT.semibold)}>× {reliability.multiplier.toFixed(4)}</td>
              </tr>
            </tbody>
          </table>
        </div>
      )}
      {reliability.floored && (
        <p data-testid="reliability-floor" className={TYPE.body}>
          The groups above multiply to less than {floor}. Together they never take the score below{' '}
          {floor} of what the tests earned, so reliability stays at {floor}. A critical defect still
          multiplies on top.
        </p>
      )}
      {unlined.map(([tier, names]) => (
        <p key={tier} data-testid="reliability-folded" className={TYPE.bodyMuted}>
          Already counted in {groupName(tier)}: {names.map(humanize).join(', ')}.
        </p>
      ))}
      {reliability.priced_as_critical.length > 0 && (
        <p data-testid="reliability-critical" className={TYPE.bodyMuted}>
          Priced as a critical defect instead, so no group counts them:{' '}
          {reliability.priced_as_critical.map(humanize).join(', ')}.
        </p>
      )}
    </section>
  );
}

/** Forge 1.0's weighted tiers (L…A) and the E slice, from the verdict — weights read, never restated. Frozen
 *  history: its table and its recorded-inputs line stay as that era was released (there "inner score" and
 *  "before criticals" are two numbers; Forge 2.0, where they are one, has ForgeTestsEarned instead). */
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
                <td className={cell}>{weightText(row.weight)}</td>
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
  // The scorer the caller names, else the one the verdict recorded: every family test below reads it.
  const version =
    scorerVersion ?? ('scorerVersion' in rawVerdict ? rawVerdict.scorerVersion : undefined);
  const sb8 = isSb8(version ?? '');
  const sb8Verdict = sb8 ? (rawVerdict as Sb8VerdictDetail) : null;
  const paymentsTier =
    isolatedPaymentsTier(scorerVersion) ??
    isolatedPaymentsTier('scorerVersion' in rawVerdict ? rawVerdict.scorerVersion : undefined);
  const forge = isForge(version);
  // Forge 2.0 (the reliability era) explains its score top-down under its own step names; Forge 1.0 is
  // frozen history and keeps the layout it was released with.
  const forge2 = forgeHasReliability(version);
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
        ) : forge2 ? (
          // The line of steps, then each step explained in the same order: what the tests earned, the two
          // multipliers, then the cap and the final score.
          <>
            <ForgeVerdictFacts verdict={verdict} scorerVersion={version} />
            <ForgeScoreSteps verdict={verdict} score={score} scorerVersion={version} />
            <ForgeTestsEarned verdict={verdict} />
            <ForgeCriticalDefects verdict={verdict} />
            <ForgeReliability verdict={verdict} scorerVersion={version} />
            <ForgeAdmission
              admission={verdict.admission}
              rawScore={verdict.rawScore}
              score={score}
              forge2
            />
          </>
        ) : forge ? (
          <>
            <ForgeVerdictFacts verdict={verdict} scorerVersion={version} />
            <ForgeAdmission
              admission={verdict.admission}
              rawScore={verdict.rawScore}
              score={score}
              forge2={false}
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

      {/* `root_causes` means a different thing per scorer, so each family gets only what is true of it.
          The sb-5 lineage zeroed a root's dependents and counted the root once — its sentence stays.
          A Forge scorer lists a root and its dependents whenever each scored below 1 (not failed, not
          zeroed), and every one of them still counts at its own score: Forge 1.0's history says exactly
          that. Forge 2.0 says nothing: under its per-group reliability a root explains no number on this
          screen (ROOT_BLOCKS only shadows one critical defect under another, which the critical-defect
          lines state), and leanzero.net shows no such block either. */}
      {rootCauses.length > 0 && !forge2 && (
        <section className={cx(SURFACE.card, SPACE.card)}>
          <SectionHeader
            as="h3"
            title={forge ? 'Tests that can share a cause' : 'Root-cause attribution'}
          />
          {rootCauses.map(([root, downstream]) => {
            // The Forge scorer repeats a name a root reaches by two routes.
            const names = [...new Set(downstream)];
            return forge ? (
              <p key={root} className={cx('mt-2', TYPE.body)}>
                <span className={WEIGHT.semibold}>{humanize(root)}</span> scored below full marks,
                and so did {names.length} test{names.length === 1 ? '' : 's'} that the same fault
                can fail: {names.map(humanize).join(', ')}. Each keeps its own score.
              </p>
            ) : (
              <p key={root} className={cx('mt-2', TYPE.body)}>
                <span className={cx(WEIGHT.semibold, TONE_TEXT.err)}>{humanize(root)}</span> failed
                at the root and zeroed {downstream.length} downstream check
                {downstream.length === 1 ? '' : 's'}: {downstream.map(humanize).join(', ')} — one
                defect, not {downstream.length + 1}.
              </p>
            );
          })}
        </section>
      )}

      <div className="flex flex-col gap-3">
        {groups.map((g) => (
          <TierGroup
            key={g.tier}
            tier={g.tier}
            sb8={sb8}
            forge={forge}
            decimals={scoresAsDecimals(version)}
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
