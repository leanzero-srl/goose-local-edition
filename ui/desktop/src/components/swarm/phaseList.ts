import type { PhaseKey, TurnLane } from './useSwarmRun';

/**
 * The phases that happen BEFORE a worker builds anything — the PLANNING zone's half of the pipeline; build,
 * integrate and repair are the work board. Kept as one list because the panel used to hand-type four keys
 * into a filter, and a phase added to the checklist (ASK, CONTRACTS) then rendered in neither zone.
 * `research` is LIVE again (the v2 fan: the opener's own questions answered read-only across the fleet
 * between ASK and SYNTHESIS); `contracts` (deleted by P1-4) and `review` (the LLM review round, deleted
 * by 2447d145c) are RETIRED but stay in this filter so an ARCHIVED run's historical rows and lanes still
 * land in the planning zone — the keys cost a new run nothing because its checklist never creates those
 * phases' items.
 */
export const PLANNING_PHASE_KEYS: ReadonlyArray<PhaseKey> = [
  'open',
  'ask',
  'research',
  'synthesis',
  'review',
  'contracts',
  'split',
];

export function isPlanningPhase(key: PhaseKey): boolean {
  return PLANNING_PHASE_KEYS.includes(key);
}

export interface PhaseLaneGroup {
  label: string;
  lanes: TurnLane[];
}

/** The single-node planning calls each planning step OWNS, by digest key (VA-138: "each step shows
 *  its LANES"). `open` fans as open-coverage-*, ASK's proxy answer is proxy-answer, SYNTHESIS is the
 *  one `synthesis` call, THE SPLIT is `split-<task>` (one per fat task), and the archived REVIEW round
 *  fanned as review-*. A key no step claims (`rate`) stays in the trailing planning-calls group. */
const PLANNING_LANE_OWNER: ReadonlyArray<{ key: PhaseKey; owns: (taskId: string) => boolean }> = [
  {
    key: 'open',
    owns: (id) => id === 'open' || id === 'open-resplit' || id.startsWith('open-coverage-'),
  },
  { key: 'ask', owns: (id) => id === 'proxy-answer' },
  { key: 'synthesis', owns: (id) => id === 'synthesis' },
  { key: 'split', owns: (id) => id.startsWith('split-') },
  { key: 'review', owns: (id) => id === 'review' || id.startsWith('review-') },
];

const PLANNING_LANE_LABEL: Partial<Record<PhaseKey, string>> = {
  open: 'Opener calls',
  ask: 'Proxy answer',
  synthesis: 'Synthesis call',
  split: 'Split calls',
  review: 'Review calls',
};

/** The planning calls no step claims — what the trailing "Planning calls" group renders once every
 *  step has taken its own. */
export function unclaimedPlanningLanes(planningLanes: TurnLane[]): TurnLane[] {
  return planningLanes.filter((l) => !PLANNING_LANE_OWNER.some((o) => o.owns(l.taskId)));
}

/**
 * The fleet fan that belongs to a planning phase, so its lanes render UNDER that phase's checklist rather than
 * in a trailing group that says nothing about when they ran. RESEARCH owns TWO fans across engine versions:
 * v2's research-* question lanes (the live engine — their own fold group since panel #5) and v1's slice fan
 * (archived runs only); a run produces one or the other, never both, so the live group wins when it exists.
 * CONTRACTS is the contract-* fan — which had no home in the zone at all and surfaced only as three busy
 * nodes parked under a ribbon lit on Build. This mapping is how an ARCHIVED run's fans keep their home.
 * Every other planning step owns its single-node calls (PLANNING_LANE_OWNER) when the caller passes
 * `planningLanes` — the r6j split lane ran 23 minutes in the trailing "Planning calls" group under a
 * Synthesize chip, listed nowhere that said WHICH step it was (VA-138).
 */
export function planningLanesFor(
  key: PhaseKey,
  lanes: {
    sliceLanes: TurnLane[];
    contractLanes: TurnLane[];
    researchLanes: TurnLane[];
    planningLanes?: TurnLane[];
  }
): PhaseLaneGroup | null {
  if (key === 'research') {
    return lanes.researchLanes.length > 0
      ? { label: 'Research answers', lanes: lanes.researchLanes }
      : { label: 'Slice specs', lanes: lanes.sliceLanes };
  }
  if (key === 'contracts') return { label: 'Contract stubs', lanes: lanes.contractLanes };
  const owner = PLANNING_LANE_OWNER.find((o) => o.key === key);
  if (owner && lanes.planningLanes) {
    const owned = lanes.planningLanes.filter((l) => owner.owns(l.taskId));
    return { label: PLANNING_LANE_LABEL[key] ?? 'Planning calls', lanes: owned };
  }
  return null;
}
