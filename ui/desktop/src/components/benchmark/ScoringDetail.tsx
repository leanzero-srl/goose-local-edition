import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';

/**
 * The full scoring story behind the single number — the sb-5.2 composition formula with each
 * component's real contribution, every check the scorer ran with its evidence string verbatim,
 * the findings that survived the repair waves, and the round-by-round repair progression.
 *
 * Everything here is scorer/engine truth persisted with the result (main.ts `verdict`); nothing
 * is re-derived from a model. Solid saturated fills, full borders, custom accordion — never a
 * left rail, a faded wash, or a native control.
 */

export interface VerdictCheck {
  check: string;
  tier: string;
  score: number;
  detail?: string;
  consequence?: string;
  parts?: Record<string, unknown>;
}

export interface VerdictDetail {
  checks: VerdictCheck[];
  tiers: Record<string, { mean: number; checks: number; weight: number }>;
  core?: number;
  hard?: number;
  excellent?: boolean;
  solid?: boolean;
  root_causes?: Record<string, string[]>;
  findingsHeld?: string[];
  repairRounds?: Array<{ round: number; findings: number }>;
}

const TIER_ORDER = ['A', 'B', 'C', 'D', 'J', 'V', 'P'] as const;

const TIER_INFO: Record<string, { name: string; desc: string; color: string }> = {
  A: {
    name: 'Structure',
    desc: 'The files and structure the spec names',
    color: 'var(--color-node-1)',
  },
  B: {
    name: 'Behaviour',
    desc: 'Does the app DO what the spec says — probed by running it',
    color: 'var(--color-node-2)',
  },
  C: {
    name: 'Vendor contract',
    desc: 'The vendor API contract — sync, idempotency, conditional fetch',
    color: 'var(--color-node-4)',
  },
  D: {
    name: 'Finesse',
    desc: 'Formats, edge cases, polish',
    color: 'var(--color-node-5)',
  },
  J: {
    name: 'Journey',
    desc: 'The user journey in a real browser',
    color: 'var(--color-node-3)',
  },
  V: {
    name: 'Visual',
    desc: 'Visual/design quality of the served page',
    color: 'var(--color-node-6)',
  },
  P: {
    name: 'Performance',
    desc: 'Measured performance budgets',
    color: 'var(--color-block-teal)',
  },
};

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

const GOOD = '#2ecc71';
const PARTIAL = '#f5a623';
const BAD = '#e5484d';

const scoreColor = (s: number) => (s >= 1 ? GOOD : s <= 0 ? BAD : PARTIAL);

const humanize = (s: string) => {
  const t = s.replace(/_/g, ' ').trim();
  return t.charAt(0).toUpperCase() + t.slice(1);
};

const pct = (v: number, digits = 0) => `${(v * 100).toFixed(digits)}%`;

function ScoreChip({ score }: { score: number }) {
  return (
    <span
      className="inline-flex w-12 shrink-0 items-center justify-center rounded px-1.5 py-0.5 text-xs font-extrabold tabular-nums"
      style={{
        backgroundColor: scoreColor(score),
        color: score > 0 && score < 1 ? '#1a1a1a' : '#fff',
      }}
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
            <span
              key={key}
              className="rounded px-1.5 py-0.5 text-[10px] font-bold text-white"
              style={{ backgroundColor: value ? GOOD : BAD }}
            >
              {value ? '✓' : '✗'} {key}
            </span>
          );
        }
        const shown =
          typeof value === 'number'
            ? Number.isInteger(value)
              ? String(value)
              : value.toFixed(2)
            : String(value).slice(0, 40);
        return (
          <span
            key={key}
            className="rounded border border-border-primary px-1.5 py-0.5 text-[10px] font-semibold tabular-nums text-text-primary"
          >
            {key} {shown}
          </span>
        );
      })}
    </div>
  );
}

function CheckRow({ check }: { check: VerdictCheck }) {
  return (
    <div className="flex items-start gap-3 border-t border-border-primary px-3 py-2.5">
      <ScoreChip score={check.score} />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[13px] font-semibold text-text-primary">
            {humanize(check.check)}
          </span>
          {HARD_CHECKS.has(check.check) && (
            <span
              className="rounded px-1.5 py-0.5 text-[9px] font-extrabold tracking-wider text-[#1a1a1a]"
              style={{ backgroundColor: PARTIAL }}
              title="Scored in the standalone 10% hard block, not this tier's mean"
            >
              HARD 10%
            </span>
          )}
        </div>
        {check.detail && (
          <div className="mt-0.5 break-words font-mono text-[11px] leading-relaxed text-text-secondary">
            {check.detail}
          </div>
        )}
        {check.score < 1 && check.consequence && (
          <div className="mt-0.5 text-[11px] font-semibold" style={{ color: BAD }}>
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
  open,
  onToggle,
}: {
  tier: string;
  checks: VerdictCheck[];
  mean: number | null;
  weight: number | null;
  open: boolean;
  onToggle: () => void;
}) {
  const info = TIER_INFO[tier] ?? { name: tier, desc: '', color: 'var(--color-node-1)' };
  const lost = checks.filter((c) => c.score < 1).length;
  return (
    <div className="overflow-hidden rounded border border-border-primary">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={open}
        className="flex w-full items-center gap-3 bg-background-secondary px-3 py-2.5 text-left"
      >
        {open ? (
          <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary" />
        ) : (
          <ChevronRight className="h-4 w-4 shrink-0 text-text-secondary" />
        )}
        <span
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-xs font-extrabold text-white"
          style={{ backgroundColor: info.color }}
        >
          {tier}
        </span>
        <span className="min-w-0 flex-1">
          <span className="text-sm font-bold text-text-primary">{info.name}</span>
          <span className="ml-2 hidden text-xs text-text-secondary sm:inline">{info.desc}</span>
        </span>
        {lost > 0 && (
          <span
            className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-extrabold text-white"
            style={{ backgroundColor: BAD }}
          >
            {lost} lost point{lost > 1 ? 's' : ''}
          </span>
        )}
        <span className="shrink-0 text-xs font-semibold tabular-nums text-text-secondary">
          {checks.length} checks{weight != null ? ` · ${pct(weight)}` : ''}
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
 * component's maximum share of the 100; the solid fill is what this build actually earned) plus
 * the arithmetic, so the final number is reproducible by eye.
 */
function CompositionBar({ verdict, score }: { verdict: VerdictDetail; score: number }) {
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
    color: string;
  }
  const components: CompComponent[] = [
    { key: 'core', label: 'Core build', value: core, weight: 0.6, color: 'var(--color-block-teal)' },
    { key: 'J', label: 'Journey', value: t.J?.mean, weight: 0.15, color: 'var(--color-node-3)' },
    { key: 'V', label: 'Visual', value: t.V?.mean, weight: 0.1, color: 'var(--color-node-6)' },
    { key: 'P', label: 'Performance', value: t.P?.mean, weight: 0.05, color: 'var(--color-node-2)' },
    {
      key: 'hard',
      label: 'Hard block',
      value: verdict.hard ?? t.HARD?.mean,
      weight: 0.1,
      color: PARTIAL,
    },
  ].filter((c): c is CompComponent => typeof c.value === 'number');

  const width = 860;
  const barH = 26;
  let x = 0;
  return (
    <div className="overflow-x-auto">
      <div className="min-w-[680px]">
        <svg width={width} height={barH + 20} role="img" aria-label="Score composition">
          {components.map((c) => {
            const slot = c.weight * width;
            const fill = Math.max(2, slot * Math.min(1, Math.max(0, c.value)));
            const g = (
              <g key={c.key}>
                <rect
                  x={x}
                  y={0}
                  width={slot}
                  height={barH}
                  fill="var(--color-background-secondary)"
                  stroke="var(--color-border-primary)"
                />
                <rect x={x} y={0} width={fill} height={barH} fill={c.color} />
                <text
                  x={x + slot / 2}
                  y={barH + 14}
                  textAnchor="middle"
                  className="fill-[var(--color-text-secondary)]"
                  style={{ fontSize: 10, fontWeight: 700 }}
                >
                  {pct(c.weight)}
                </text>
              </g>
            );
            x += slot;
            return g;
          })}
        </svg>

        <table className="mt-2 w-full border-collapse text-[12px]">
          <thead>
            <tr className="text-left text-[10px] font-extrabold uppercase tracking-wider text-text-secondary">
              <th className="border border-border-primary px-2 py-1">Component</th>
              <th className="border border-border-primary px-2 py-1">Earned</th>
              <th className="border border-border-primary px-2 py-1">Weight</th>
              <th className="border border-border-primary px-2 py-1">Points of 100</th>
            </tr>
          </thead>
          <tbody className="tabular-nums">
            {components.map((c) => (
              <tr key={c.key}>
                <td className="border border-border-primary px-2 py-1 font-semibold text-text-primary">
                  <span
                    className="mr-2 inline-block h-3 w-3 rounded-[2px] align-middle"
                    style={{ backgroundColor: c.color }}
                  />
                  {c.label}
                </td>
                <td className="border border-border-primary px-2 py-1">{pct(c.value, 1)}</td>
                <td className="border border-border-primary px-2 py-1">× {pct(c.weight)}</td>
                <td className="border border-border-primary px-2 py-1 font-bold text-text-primary">
                  {(c.value * c.weight * 100).toFixed(1)}
                </td>
              </tr>
            ))}
            <tr>
              <td
                className="border border-border-primary px-2 py-1 font-extrabold text-text-primary"
                colSpan={3}
              >
                Final score
              </td>
              <td className="border border-border-primary px-2 py-1 text-sm font-extrabold text-[var(--color-block-teal)]">
                {(score * 100).toFixed(1)}
              </td>
            </tr>
          </tbody>
        </table>

        <p className="mt-2 text-xs text-text-secondary">
          Core build = A structure × 25% + B behaviour × 30% + C vendor contract × 25% + D finesse
          × 20%
          {(['A', 'B', 'C', 'D'] as const).every((k) => typeof t[k]?.mean === 'number') && (
            <>
              {' '}
              = {(['A', 'B', 'C', 'D'] as const)
                .map((k) => `${pct(t[k].mean, 0)}·${pct(t[k].weight)}`)
                .join(' + ')}{' '}
              = <span className="font-bold text-text-primary">{pct(core, 1)}</span>
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

function RepairStrip({ rounds }: { rounds: Array<{ round: number; findings: number }> }) {
  if (rounds.length === 0) return null;
  return (
    <div>
      <div className="flex flex-wrap items-center gap-2">
        {rounds.map((r, i) => (
          <span key={r.round} className="flex items-center gap-2">
            {i > 0 && <span className="text-sm font-bold text-text-secondary">→</span>}
            <span
              className="rounded px-2 py-1 text-xs font-extrabold text-white tabular-nums"
              style={{ backgroundColor: r.findings === 0 ? GOOD : BAD }}
            >
              Round {r.round} · {r.findings} finding{r.findings === 1 ? '' : 's'}
            </span>
          </span>
        ))}
      </div>
      <p className="mt-2 max-w-[80ch] text-xs text-text-secondary">
        At each round the completion gate runs the app, opens the page in a real browser, and files
        findings; the swarm's repair waves fix them and the gate re-verifies — until it reads clean,
        the count stops moving, or the round budget is spent.
      </p>
    </div>
  );
}

export function ScoringDetail({ verdict, score }: { verdict: VerdictDetail; score: number }) {
  const groups = useMemo(() => {
    const byTier = new Map<string, VerdictCheck[]>();
    for (const c of verdict.checks) {
      const list = byTier.get(c.tier) ?? [];
      list.push(c);
      byTier.set(c.tier, list);
    }
    return TIER_ORDER.filter((t) => (byTier.get(t) ?? []).length > 0).map((t) => ({
      tier: t as string,
      checks: byTier.get(t) ?? [],
      mean: typeof verdict.tiers[t]?.mean === 'number' ? verdict.tiers[t].mean : null,
      weight: typeof verdict.tiers[t]?.weight === 'number' ? verdict.tiers[t].weight : null,
    }));
  }, [verdict]);

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
      <CompositionBar verdict={verdict} score={score} />

      {findingsHeld.length > 0 && (
        <div className="rounded border-2 px-4 py-3" style={{ borderColor: BAD }}>
          <div className="text-xs font-extrabold uppercase tracking-widest" style={{ color: BAD }}>
            Findings that held — the gate still saw these when verification ended
          </div>
          <div className="mt-2 flex flex-col gap-2">
            {findingsHeld.map((f, i) => (
              <p
                key={i}
                className="whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-text-primary"
              >
                {f}
              </p>
            ))}
          </div>
        </div>
      )}

      {(verdict.repairRounds ?? []).length > 0 && (
        <div>
          <h3 className="mb-2 text-xs font-bold uppercase tracking-widest text-text-secondary">
            Repair progression
          </h3>
          <RepairStrip rounds={verdict.repairRounds ?? []} />
        </div>
      )}

      {rootCauses.length > 0 && (
        <div className="rounded border border-border-primary px-4 py-3">
          <h3 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
            Root-cause attribution
          </h3>
          {rootCauses.map(([root, downstream]) => (
            <p key={root} className="mt-2 text-xs text-text-primary">
              <span className="font-bold" style={{ color: BAD }}>
                {humanize(root)}
              </span>{' '}
              failed at the root and zeroed {downstream.length} downstream check
              {downstream.length === 1 ? '' : 's'}: {downstream.map(humanize).join(', ')} — one
              defect, not {downstream.length + 1}.
            </p>
          ))}
        </div>
      )}

      <div className="flex flex-col gap-3">
        {groups.map((g) => (
          <TierGroup
            key={g.tier}
            tier={g.tier}
            checks={g.checks}
            mean={g.mean}
            weight={g.weight}
            open={!!open[g.tier]}
            onToggle={() => setOpen((o) => ({ ...o, [g.tier]: !o[g.tier] }))}
          />
        ))}
      </div>
    </div>
  );
}
