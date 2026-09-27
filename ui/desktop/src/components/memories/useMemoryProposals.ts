import { useCallback, useEffect, useRef, useState } from 'react';
import { acpAnswerMemoryProposal, acpListMemoryProposals } from '../../acp/proposals';
import type { MemoryProposalDto } from '../../acp/proposals';
import { ChatState } from '../../types/chatState';

/** The assessment is a detached engine task that lands a few seconds after the turn ends, so the
 *  card polls after every Idle transition: at once, then every POLL_MS for POLL_TRIES. */
export const POLL_MS = 3000;
export const POLL_TRIES = 12;

/**
 * A card holds this many characters — the store's `PROPOSAL_TEXT_MAX_CHARS`
 * (goose-memory-store proposals.rs), which the card's editor also holds to.
 */
export const PROPOSAL_CARD_MAX_CHARS = 350;

/**
 * A proposal filed before Q-93 had its text CUT at the card's limit — the store clamped instead of
 * refusing (the Sep 25 card ends `by exception only". Bi`). Since Q-93 a text over the card is
 * refused whole, so an open text at the limit is that clamp's stump: it is shown expired, never
 * offered for Save (Save would store the stump).
 */
export function isClampedStump(p: MemoryProposalDto): boolean {
  return p.state === 'open' && [...p.text].length >= PROPOSAL_CARD_MAX_CHARS;
}

/**
 * What the card shows: open proposals, and expired ones (rendered as "expired — not saved").
 * A card filed for the whole project (`key` is not this chat's) is shown only while it is not
 * older than the chat's latest question: a days-old project card pinned under every new question
 * read as the reply (Q-172). `lastQuestionAt` (seconds) null = no question yet, nothing to hide.
 */
export function visibleProposals(
  all: MemoryProposalDto[],
  sessionId?: string,
  lastQuestionAt: number | null = null
): MemoryProposalDto[] {
  return all.filter(
    (p) =>
      (p.state === 'open' || p.state === 'expired') &&
      (p.key === sessionId || lastQuestionAt == null || p.createdAt >= lastQuestionAt)
  );
}

export function useMemoryProposals(
  sessionId: string | undefined,
  chatState: ChatState,
  lastQuestionAt: number | null = null
) {
  const [proposals, setProposals] = useState<MemoryProposalDto[]>([]);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);

  const refresh = useCallback(async () => {
    if (!sessionId) return [] as MemoryProposalDto[];
    try {
      const all = await acpListMemoryProposals(sessionId);
      setProposals(visibleProposals(all, sessionId, lastQuestionAt));
      return all;
    } catch {
      // Fail open: a read that fails leaves the transcript as it was.
      return [] as MemoryProposalDto[];
    }
  }, [sessionId, lastQuestionAt]);

  useEffect(() => {
    if (!sessionId || chatState !== ChatState.Idle) return;
    const mine = ++generation.current;
    let tries = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const before = proposals.filter((p) => p.state === 'open').length;
    const tick = async () => {
      if (generation.current !== mine) return;
      const all = await refresh();
      tries += 1;
      const open = all.filter((p) => p.state === 'open').length;
      // Stop once a new card has landed, or after the window has passed.
      if (open > before || tries >= POLL_TRIES) return;
      timer = setTimeout(() => void tick(), POLL_MS);
    };
    void tick();
    return () => {
      generation.current += 1;
      if (timer) clearTimeout(timer);
    };
    // `proposals` is deliberately not a dependency: the baseline is read once per Idle transition.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, chatState, refresh]);

  const answer = useCallback(
    async (proposal: MemoryProposalDto, decision: 'save' | 'decline', text?: string) => {
      if (!sessionId) return;
      setBusyId(proposal.id);
      setError(null);
      try {
        await acpAnswerMemoryProposal(sessionId, proposal, decision, text);
        setProposals((prev) => prev.filter((p) => p.id !== proposal.id));
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusyId(null);
      }
    },
    [sessionId]
  );

  const dismissExpired = useCallback((id: string) => {
    setProposals((prev) => prev.filter((p) => p.id !== id));
  }, []);

  return { proposals, busyId, error, answer, dismissExpired, refresh };
}
