import { useCallback, useEffect, useRef, useState } from 'react';
import { acpAnswerMemoryProposal, acpListMemoryProposals } from '../../acp/proposals';
import type { MemoryProposalDto } from '../../acp/proposals';
import { ChatState } from '../../types/chatState';

/** The assessment is a detached engine task that lands a few seconds after the turn ends, so the
 *  card polls after every Idle transition: at once, then every POLL_MS for POLL_TRIES. */
export const POLL_MS = 3000;
export const POLL_TRIES = 12;

/** What the card shows: open proposals, and expired ones (rendered as "expired — not saved"). */
export function visibleProposals(all: MemoryProposalDto[]): MemoryProposalDto[] {
  return all.filter((p) => p.state === 'open' || p.state === 'expired');
}

export function useMemoryProposals(sessionId: string | undefined, chatState: ChatState) {
  const [proposals, setProposals] = useState<MemoryProposalDto[]>([]);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);

  const refresh = useCallback(async () => {
    if (!sessionId) return [] as MemoryProposalDto[];
    try {
      const all = await acpListMemoryProposals(sessionId);
      setProposals(visibleProposals(all));
      return all;
    } catch {
      // Fail open: a read that fails leaves the transcript as it was.
      return [] as MemoryProposalDto[];
    }
  }, [sessionId]);

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
