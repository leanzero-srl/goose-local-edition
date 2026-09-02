import { useEffect, useState } from 'react';
import { X, Repeat, Sparkles, Wand2, Play, ArrowRight } from 'lucide-react';
import { toast } from 'react-toastify';
import { LeanZero } from '../icons';
import { LoopModal, type NewLoopPayload } from '../loop/LoopModal';
import { acpCreateSchedule, acpListSchedules, acpRunScheduleNow } from '../../acp/schedules';
import { listSkillSources } from '../../acp/sources';
import RecipeWizard from './RecipeWizard';
import RecipeChatWizard from './RecipeChatWizard';
import { getInitialWorkingDir } from '../../utils/workingDir';
import type { setViewType } from '../../hooks/useNavigation';
import {
  Button,
  FOCUS,
  MOTION,
  RADIUS,
  SURFACE,
  TONE_FILL,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';

/**
 * Goose Local Edition — the Agent persona setup wizard. The autonomous Agent runs a LOOP built from a
 * RECIPE, with SKILLS it can call. Ties the app's Loop / Recipe / Skills features together, offering both
 * ways the user asked for to make a recipe:
 *  - Build a recipe WITH THE FLEET (RecipeChatWizard) — a warm local model interviews the user and drafts
 *    it. The swarm *provider* is a build orchestrator and can't hold a conversation, so this talks to LM
 *    Studio directly instead.
 *  - Or draft one BY HAND (RecipeWizard) — fill the fields in a form.
 *  - Create a loop from a recipe (LoopModal — recipe + schedule + iterations).
 *  - Skills surface as /commands; link out to manage them.
 * Studio chrome: the one overlay elevation, hairline dividers, section headers in the zone register, the
 * accent fill on the one leading action, solid fills for every state (no left rail, no faded tints).
 */

export function AgentSetupWizard({
  isOpen,
  onClose,
  setView,
  workingDir,
}: {
  isOpen: boolean;
  onClose: () => void;
  setView: setViewType;
  workingDir?: string;
}) {
  const [loopModalOpen, setLoopModalOpen] = useState(false);
  const [recipeWizardOpen, setRecipeWizardOpen] = useState(false);
  const [recipeChatOpen, setRecipeChatOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [createdLoopId, setCreatedLoopId] = useState<string | null>(null);
  const [skillCount, setSkillCount] = useState<number | null>(null);
  const [loops, setLoops] = useState<{ id: string }[]>([]);

  const dir = workingDir || getInitialWorkingDir();

  useEffect(() => {
    if (!isOpen) return;
    let alive = true;
    void (async () => {
      try {
        const sources = await listSkillSources(dir);
        if (alive) setSkillCount(sources.length);
      } catch {
        if (alive) setSkillCount(null);
      }
      try {
        const jobs = (await acpListSchedules()) as Array<{ id: string; loopConfig?: unknown }>;
        if (alive) setLoops(jobs.filter((j) => j.loopConfig != null).map((j) => ({ id: j.id })));
      } catch {
        /* leave loops empty */
      }
    })();
    return () => {
      alive = false;
    };
  }, [isOpen, dir]);

  if (!isOpen) return null;

  const handleCreateLoop = async (payload: NewLoopPayload) => {
    setSubmitting(true);
    setError(null);
    try {
      const job = (await acpCreateSchedule(payload)) as { id?: string };
      const id = job.id ?? payload.id;
      setCreatedLoopId(id);
      setLoops((prev) => [{ id }, ...prev.filter((l) => l.id !== id)]);
      setLoopModalOpen(false);
      toast.success(`Loop "${id}" created`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  const runNow = async (id: string) => {
    try {
      await acpRunScheduleNow(id);
      toast.success(`Agent "${id}" started`);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const goToSkills = () => {
    setView('skills');
    onClose();
  };

  return (
    <>
      {/* Hidden while a child modal (loop / by-hand recipe / fleet chat) is open so this z-70 overlay isn't in the way. */}
      {!loopModalOpen && !recipeWizardOpen && !recipeChatOpen && (
        <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/50 p-4">
        <div
          className={cx('flex w-[540px] max-h-[85vh] flex-col', SURFACE.overlay)}
          data-testid="agent-setup-wizard"
        >
          <div className="flex items-center justify-between border-b border-lz-border px-4 py-3">
            <div className="flex items-center gap-2">
              <span className={cx('flex h-6 w-6 items-center justify-center', RADIUS.control, TONE_FILL.accent)}>
                <LeanZero className="h-4 w-4 text-white" />
              </span>
              <h3 className={TYPE.h2}>Set up the autonomous Agent</h3>
            </div>
            <button
              onClick={onClose}
              className={cx('text-lz-ink-3 hover:text-lz-ink', MOTION, FOCUS)}
              aria-label="Close"
            >
              <X className="h-4 w-4" />
            </button>
          </div>

          <div className="space-y-3 overflow-y-auto px-4 py-3">
            <p className={TYPE.bodyMuted}>
              The Agent runs on its own: a <span className={cx('text-lz-ink', WEIGHT.medium)}>loop</span> built from
              a <span className={cx('text-lz-ink', WEIGHT.medium)}>recipe</span>, iterating across your fleet, with{' '}
              <span className={cx('text-lz-ink', WEIGHT.medium)}>skills</span> it can call. Build a recipe by
              chatting with your fleet, or fill one in by hand, then wrap it in a loop.
            </p>

            {/* Recipe + Loop */}
            <div className={cx('overflow-hidden border border-lz-border', RADIUS.control)}>
              <div className={cx('flex items-center gap-1.5 border-b border-lz-border bg-lz-surface-2 px-3 py-1.5', TYPE.zone)}>
                <Repeat className="h-3.5 w-3.5" /> Loop &amp; recipe
              </div>
              <div className="space-y-2 p-3">
                <button
                  onClick={() => setRecipeChatOpen(true)}
                  className={cx(
                    'flex w-full items-center justify-between px-3 py-2 text-lz-body hover:bg-lz-accent-hover',
                    WEIGHT.medium,
                    TONE_FILL.accent,
                    RADIUS.control,
                    MOTION,
                    FOCUS
                  )}
                >
                  <span className="flex items-center gap-2">
                    <Sparkles className="h-4 w-4" /> Build a recipe with the fleet (chat)
                  </span>
                  <ArrowRight className="h-4 w-4" />
                </button>
                <button
                  onClick={() => setRecipeWizardOpen(true)}
                  className={cx('flex w-full items-center justify-between border border-lz-border-strong bg-lz-surface px-3 py-2 text-lz-body text-lz-ink', SURFACE.hover, RADIUS.control, MOTION, FOCUS)}
                >
                  <span className="flex items-center gap-2">
                    <Wand2 className="h-4 w-4" /> …or draft one by hand
                  </span>
                  <ArrowRight className="h-4 w-4" />
                </button>
                <button
                  onClick={() => setLoopModalOpen(true)}
                  className={cx('flex w-full items-center justify-between border border-lz-border-strong bg-lz-surface px-3 py-2 text-lz-body text-lz-ink', SURFACE.hover, RADIUS.control, MOTION, FOCUS)}
                >
                  <span className="flex items-center gap-2">
                    <Repeat className="h-4 w-4" /> Create a loop (recipe + schedule + iterations)
                  </span>
                  <ArrowRight className="h-4 w-4" />
                </button>

                {createdLoopId && (
                  <div className={cx('flex items-center justify-between border border-lz-ok px-3 py-2', RADIUS.control)}>
                    <span className="truncate text-lz-body text-lz-ink">
                      Loop <span className="font-mono">{createdLoopId}</span> ready
                    </span>
                    <button
                      onClick={() => runNow(createdLoopId)}
                      className={cx('flex items-center gap-1 px-2 py-1 text-lz-meta', WEIGHT.semibold, TONE_FILL.ok, RADIUS.control, MOTION, FOCUS)}
                    >
                      <Play className="h-3.5 w-3.5" /> Run agent now
                    </button>
                  </div>
                )}

                {loops.length > 0 && (
                  <div className="pt-1">
                    <div className="mb-1 text-lz-meta text-lz-ink-3">Existing loops</div>
                    <div className="space-y-1">
                      {loops.slice(0, 5).map((l) => (
                        <div key={l.id} className="flex items-center justify-between text-lz-meta">
                          <span className="truncate font-mono text-lz-mono text-lz-ink">{l.id}</span>
                          <button
                            onClick={() => runNow(l.id)}
                            className={cx('flex items-center gap-1 text-lz-ink-3 hover:text-lz-ink', MOTION, FOCUS)}
                          >
                            <Play className="h-3.5 w-3.5" /> Run
                          </button>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            </div>

            {/* Skills */}
            <div className={cx('overflow-hidden border border-lz-border', RADIUS.control)}>
              <div className={cx('flex items-center gap-1.5 border-b border-lz-border bg-lz-surface-2 px-3 py-1.5', TYPE.zone)}>
                <Sparkles className="h-3.5 w-3.5" /> Skills
              </div>
              <div className="flex items-center justify-between p-3">
                <span className="text-lz-meta text-lz-ink-2">
                  Skills become <span className="font-mono text-lz-ink">/commands</span> your agent can call.
                  {skillCount != null && (
                    <>
                      {' '}
                      <span className={cx('text-lz-ink', WEIGHT.medium)}>{skillCount}</span> available.
                    </>
                  )}
                </span>
                <button
                  onClick={goToSkills}
                  className={cx(
                    'flex shrink-0 items-center gap-1 border border-lz-border-strong bg-lz-surface px-2 py-1 text-lz-meta text-lz-ink',
                    SURFACE.hover,
                    RADIUS.control,
                    MOTION,
                    FOCUS
                  )}
                >
                  Manage <ArrowRight className="h-3.5 w-3.5" />
                </button>
              </div>
            </div>

            {error && (
              <div className={cx('border border-lz-err px-3 py-2 text-lz-meta', TONE_TEXT.err, RADIUS.control)}>
                {error}
              </div>
            )}
          </div>

          <div className="flex justify-end border-t border-lz-border px-4 py-3">
            <Button variant="secondary" size="sm" onClick={onClose}>
              Done
            </Button>
          </div>
        </div>
        </div>
      )}

      <LoopModal
        isOpen={loopModalOpen}
        onClose={() => setLoopModalOpen(false)}
        onSubmit={handleCreateLoop}
        isLoadingExternally={submitting}
        apiErrorExternally={error}
      />

      <RecipeWizard
        isOpen={recipeWizardOpen}
        onClose={() => setRecipeWizardOpen(false)}
        onSaved={() => {
          // Recipe saved: close the whole setup flow (not just this child) so the user isn't left
          // staring at the still-open setup modal after a successful creation.
          setRecipeWizardOpen(false);
          onClose();
        }}
      />

      <RecipeChatWizard
        isOpen={recipeChatOpen}
        onClose={() => setRecipeChatOpen(false)}
        onSaved={() => {
          setRecipeChatOpen(false);
          onClose();
        }}
      />
    </>
  );
}

export default AgentSetupWizard;
