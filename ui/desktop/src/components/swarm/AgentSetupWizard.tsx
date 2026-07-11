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
 * Sharp full-border modal (no left rail, no faded tints, solid azure), matching the Local Edition look.
 */

const AZURE = '#2e8bff';

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
          className="bg-background-primary border border-border-primary shadow-lg w-[540px] max-h-[85vh] flex flex-col"
          style={{ borderRadius: 3 }}
          data-testid="agent-setup-wizard"
        >
          <div className="flex items-center justify-between px-4 py-3 border-b border-border-primary">
            <div className="flex items-center gap-2">
              <span className="flex items-center justify-center h-6 w-6" style={{ backgroundColor: AZURE }}>
                <LeanZero className="h-4 w-4 text-white" />
              </span>
              <h3 className="text-sm font-semibold text-text-primary">Set up the autonomous Agent</h3>
            </div>
            <button
              onClick={onClose}
              className="text-text-secondary hover:text-text-primary"
              aria-label="Close"
            >
              <X className="h-4 w-4" />
            </button>
          </div>

          <div className="px-4 py-3 overflow-y-auto space-y-3 text-sm">
            <p className="text-text-secondary text-xs">
              The Agent runs on its own: a <span className="text-text-primary font-medium">loop</span> built from
              a <span className="text-text-primary font-medium">recipe</span>, iterating across your fleet, with{' '}
              <span className="text-text-primary font-medium">skills</span> it can call. Build a recipe by
              chatting with your fleet, or fill one in by hand, then wrap it in a loop.
            </p>

            {/* Recipe + Loop */}
            <div className="border border-border-primary" style={{ borderRadius: 3 }}>
              <div className="px-3 py-1.5 text-xs font-semibold bg-background-secondary border-b border-border-primary flex items-center gap-1.5">
                <Repeat className="h-3.5 w-3.5" /> Loop &amp; recipe
              </div>
              <div className="p-3 space-y-2">
                <button
                  onClick={() => setRecipeChatOpen(true)}
                  className="w-full flex items-center justify-between px-3 py-2 text-xs font-medium text-white transition-opacity hover:opacity-90"
                  style={{ backgroundColor: AZURE, borderRadius: 3 }}
                >
                  <span className="flex items-center gap-2">
                    <Sparkles className="h-4 w-4" /> Build a recipe with the fleet (chat)
                  </span>
                  <ArrowRight className="h-4 w-4" />
                </button>
                <button
                  onClick={() => setRecipeWizardOpen(true)}
                  className="w-full flex items-center justify-between px-3 py-2 text-xs border border-border-primary text-text-primary hover:border-text-secondary transition-colors"
                  style={{ borderRadius: 3 }}
                >
                  <span className="flex items-center gap-2">
                    <Wand2 className="h-4 w-4" /> …or draft one by hand
                  </span>
                  <ArrowRight className="h-4 w-4" />
                </button>
                <button
                  onClick={() => setLoopModalOpen(true)}
                  className="w-full flex items-center justify-between px-3 py-2 text-xs border border-border-primary text-text-primary hover:border-text-secondary transition-colors"
                  style={{ borderRadius: 3 }}
                >
                  <span className="flex items-center gap-2">
                    <Repeat className="h-4 w-4" /> Create a loop (recipe + schedule + iterations)
                  </span>
                  <ArrowRight className="h-4 w-4" />
                </button>

                {createdLoopId && (
                  <div className="flex items-center justify-between px-3 py-2 border" style={{ borderColor: '#2ecc71', borderRadius: 3 }}>
                    <span className="text-xs text-text-primary truncate">
                      Loop <span className="font-mono">{createdLoopId}</span> ready
                    </span>
                    <button
                      onClick={() => runNow(createdLoopId)}
                      className="flex items-center gap-1 text-xs font-semibold px-2 py-1 text-white"
                      style={{ backgroundColor: '#2ecc71', borderRadius: 3 }}
                    >
                      <Play className="h-3.5 w-3.5" /> Run agent now
                    </button>
                  </div>
                )}

                {loops.length > 0 && (
                  <div className="pt-1">
                    <div className="text-[11px] text-text-secondary mb-1">Existing loops</div>
                    <div className="space-y-1">
                      {loops.slice(0, 5).map((l) => (
                        <div key={l.id} className="flex items-center justify-between text-xs">
                          <span className="font-mono truncate text-text-primary">{l.id}</span>
                          <button
                            onClick={() => runNow(l.id)}
                            className="flex items-center gap-1 text-text-secondary hover:text-text-primary"
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
            <div className="border border-border-primary" style={{ borderRadius: 3 }}>
              <div className="px-3 py-1.5 text-xs font-semibold bg-background-secondary border-b border-border-primary flex items-center gap-1.5">
                <Sparkles className="h-3.5 w-3.5" /> Skills
              </div>
              <div className="p-3 flex items-center justify-between">
                <span className="text-xs text-text-secondary">
                  Skills become <span className="font-mono text-text-primary">/commands</span> your agent can call.
                  {skillCount != null && (
                    <>
                      {' '}
                      <span className="text-text-primary font-medium">{skillCount}</span> available.
                    </>
                  )}
                </span>
                <button
                  onClick={goToSkills}
                  className="shrink-0 flex items-center gap-1 text-xs border border-border-primary px-2 py-1 text-text-primary hover:border-text-secondary transition-colors"
                  style={{ borderRadius: 3 }}
                >
                  Manage <ArrowRight className="h-3.5 w-3.5" />
                </button>
              </div>
            </div>

            {error && (
              <div className="text-xs px-3 py-2 border" style={{ color: '#ff3b30', borderColor: '#ff3b30', borderRadius: 3 }}>
                {error}
              </div>
            )}
          </div>

          <div className="px-4 py-3 border-t border-border-primary flex justify-end">
            <button
              onClick={onClose}
              className="text-xs px-3 py-1.5 border border-border-primary text-text-primary hover:border-text-secondary transition-colors"
              style={{ borderRadius: 3 }}
            >
              Done
            </button>
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
