import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import real from './__fixtures__/ledgerCoreTestsRetry.json';
import { SaidSection, inspectorOutputText, laneSaidState } from './SwarmRunPanel';
import { saidKindOf, splitTranscriptAttempts, type SupersededSaid } from './useSwarmRun';

/**
 * THE r0 CASE, ON THE REAL BYTES. ledger-core-tests attempt 0 ended in the agent's transport error
 * ("Network error: Stream decode error … Please resend your message to try again.") and the SAID pane
 * showed it as the current answer for 24+ minutes while attempt 1 ran — the owner: "I can't see if
 * it's current or happened and resolved, there's no state to any of this." The fixture is cut from
 * that run's actual digest/log (see its `_why`); these tests pin the whole chain: the split at the
 * attempt markers, the deterministic error classification, the superseded chip while the new attempt
 * has said nothing, and the legacy no-marker path reading exactly as before.
 */

const digest = real.digest_mid_retry;

const midRetryLane = {
  fullTranscript: real.log_mid_retry,
  lastText: digest.last_text,
  attempt: digest.attempt,
  saidKind: digest.said_kind as 'said',
  superseded: digest.superseded as SupersededSaid[],
  error: real.retry_error,
};

describe('splitTranscriptAttempts — the durable log cut at its attempt markers', () => {
  it('splits the mid-retry log into a superseded attempt 0 and an empty live attempt 1', () => {
    const { live, superseded } = splitTranscriptAttempts(real.log_mid_retry);
    expect(live.attempt).toBe(1);
    expect(live.text.trim()).toBe('');
    expect(superseded).toHaveLength(1);
    expect(superseded[0].attempt).toBe(0);
    expect(superseded[0].text).toContain('Network error: Stream decode error');
  });

  it('legacy no-marker log reads whole as the live segment — old runs unchanged', () => {
    const { live, superseded } = splitTranscriptAttempts(real.log_legacy);
    expect(live.attempt).toBeNull();
    expect(live.text).toBe(real.log_legacy);
    expect(superseded).toHaveLength(0);
  });
});

describe('saidKindOf — the deterministic error/said rule, mirroring the engine', () => {
  it('classifies the real transport error as error and the real answer as said', () => {
    expect(saidKindOf(real.error_text)).toBe('error');
    expect(saidKindOf(real.answer_tail)).toBe('said');
  });
});

describe('laneSaidState — provenance for the pane', () => {
  it('mid-retry: attempt 1 live and empty, attempt 0 superseded as an error with the retry reason', () => {
    const s = laneSaidState(midRetryLane);
    expect(s.live).toMatchObject({ attempt: 1, text: '', kind: 'said' });
    expect(s.superseded).toHaveLength(1);
    expect(s.superseded[0]).toMatchObject({ attempt: 0, kind: 'error', retried: real.retry_error });
    expect(s.superseded[0].text).toContain('Please resend your message to try again.');
  });

  it('with no log twin (a mirror lane) the digest superseded list carries the same state', () => {
    const s = laneSaidState({ ...midRetryLane, fullTranscript: undefined });
    expect(s.live).toMatchObject({ attempt: 1, text: '' });
    expect(s.superseded).toHaveLength(1);
    expect(s.superseded[0]).toMatchObject({ attempt: 0, kind: 'error' });
  });

  it('legacy lane: no chips-worthy state, the whole log stays the live body', () => {
    const s = laneSaidState({ fullTranscript: real.log_legacy });
    expect(s.live.attempt).toBeNull();
    expect(s.superseded).toHaveLength(0);
    expect(inspectorOutputText({ fullTranscript: real.log_legacy })).toBe(real.log_legacy.trim());
  });

  it('the dead attempt never masquerades as the live body', () => {
    expect(inspectorOutputText(midRetryLane)).toBe('');
  });

  it('a reused lane key (REVIEW, every round at attempt 0) labels the old call "earlier call", not a contradiction', () => {
    const marker = (n: number) => `\n===== swarm attempt ${n} · dispatched 2026-08-29T22:00:00+00:00 =====\n`;
    const s = laneSaidState({
      fullTranscript: `${marker(0)}round 1 verdict: two findings.${marker(0)}`,
      attempt: 0,
    });
    expect(s.live.attempt).toBe(0);
    expect(s.superseded).toHaveLength(1);
    // Same attempt number as live -> null -> renders "earlier call · superseded".
    expect(s.superseded[0].attempt).toBeNull();
    expect(s.superseded[0].text).toBe('round 1 verdict: two findings.');
  });
});

describe('SaidSection — the pane says whose text it shows', () => {
  const renderMidRetry = () =>
    render(
      <SaidSection
        said={laneSaidState(midRetryLane)}
        narration={inspectorOutputText(midRetryLane)}
        processing
      />
    );

  it('shows "superseded" while attempt 1 has no text yet — never the old error as body', () => {
    renderMidRetry();
    // The live chip names the current attempt (1-based for humans, like the event feed).
    expect(screen.getByText('attempt 2 · live')).toBeInTheDocument();
    // The empty new attempt reads as processing, not as the dead attempt's error.
    expect(screen.getByText('processing the prompt…')).toBeInTheDocument();
    // The old attempt is present as SUPERSEDED, collapsed — its error text is not in the body.
    // (The closer sentence is asserted, not the error head: the "retried:" caption legitimately
    // quotes the head, while the full error bytes exist only in the collapsed body.)
    expect(screen.getByText('from attempt 1 · error → retried')).toBeInTheDocument();
    expect(screen.queryByText(/Please resend your message to try again/)).toBeNull();
    // The retry that followed is captioned from task_retry.error.
    expect(screen.getByText(`retried: ${real.retry_error}`)).toBeInTheDocument();
  });

  it('the error is styled as an ERROR state — solid, distinct, expandable to the real bytes', () => {
    renderMidRetry();
    const block = screen.getByTestId('superseded-said');
    expect(block).toHaveAttribute('data-said-kind', 'error');
    // The chip carries the SOLID error fill (never a faded tint, never a left accent stripe).
    const chip = screen.getByText('from attempt 1 · error → retried');
    expect(chip).toHaveAttribute('data-said-kind', 'error');
    expect(chip.getAttribute('style') ?? '').toContain('dc2626');
    // Expanding shows the superseded error bytes.
    fireEvent.click(screen.getByRole('button', { name: 'show' }));
    expect(screen.getByText(/Please resend your message to try again/)).toBeInTheDocument();
  });

  it('a legacy lane renders the old chipless body exactly as before', () => {
    render(
      <SaidSection
        said={laneSaidState({ fullTranscript: real.log_legacy })}
        narration={inspectorOutputText({ fullTranscript: real.log_legacy })}
        processing={false}
      />
    );
    expect(screen.queryByTestId('superseded-said')).toBeNull();
    expect(screen.queryByText(/attempt \d/)).toBeNull();
    expect(screen.getByTestId('said-section').textContent).toContain(
      "I've written comprehensive tests for ledger-core"
    );
  });
});
