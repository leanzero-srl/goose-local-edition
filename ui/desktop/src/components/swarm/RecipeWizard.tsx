import { useState, type ReactNode } from 'react';
import { X, Wand2, Loader2, Check } from 'lucide-react';
import { toast } from 'react-toastify';
import { LeanZero } from '../icons';
import type { Recipe } from '../../recipe';
import { saveRecipe } from '../../recipe/recipe_management';
import { Button, FOCUS, MOTION, RADIUS, SURFACE, TONE_FILL, TONE_TEXT, TYPE, cx } from '../lz';

/**
 * Goose Local Edition — a GUIDED recipe wizard. It ASKS the questions in the UI (what should the agent do,
 * a name, a one-line description) and assembles + saves a real recipe. This replaces the earlier "build with
 * the swarm" step, which wrongly sent a "please ask me questions" prompt to the swarm PROVIDER — but the
 * swarm is a build orchestrator, not a conversational model, so it just started a build run instead of a Q&A.
 *
 * Studio chrome: the one overlay elevation, hairline dividers, the field recipe on border-strong /
 * surface / body ink, ONE primary Button (Save).
 */

const FIELD = cx(
  'w-full border border-lz-border-strong bg-lz-surface px-2.5 py-1.5 text-lz-body text-lz-ink placeholder:text-lz-ink-3',
  RADIUS.control,
  FOCUS
);

// Defined at module scope on purpose: a component declared inside RecipeWizard would be a new type
// on every render, remounting its inputs and dropping focus after each keystroke.
function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div>
      <div className="mb-1 text-lz-body text-lz-ink">{label}</div>
      {hint && <div className="mb-1.5 text-lz-meta text-lz-ink-3">{hint}</div>}
      {children}
    </div>
  );
}

export function RecipeWizard({
  isOpen,
  onClose,
  onSaved,
}: {
  isOpen: boolean;
  onClose: () => void;
  onSaved: (recipe: Recipe) => void;
}) {
  const [instructions, setInstructions] = useState('');
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!isOpen) return null;

  const canSave = title.trim().length > 0 && instructions.trim().length > 0;

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const recipe: Recipe = {
        version: '1.0.0',
        title: title.trim(),
        description: description.trim() || title.trim(),
        instructions: instructions.trim(),
      };
      await saveRecipe(recipe, null);
      toast.success(`Recipe "${recipe.title}" saved`);
      onSaved(recipe);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[75] flex items-center justify-center bg-black/50 p-4">
      <div
        className={cx('flex w-[560px] max-h-[88vh] flex-col', SURFACE.overlay)}
        data-testid="recipe-wizard"
      >
        <div className="flex items-center justify-between border-b border-lz-border px-4 py-3">
          <div className="flex items-center gap-2">
            <span
              className={cx(
                'flex h-6 w-6 items-center justify-center',
                RADIUS.control,
                TONE_FILL.accent
              )}
            >
              <LeanZero className="h-4 w-4 text-white" />
            </span>
            <h3 className={TYPE.h2}>Draft a recipe</h3>
          </div>
          <button
            onClick={onClose}
            className={cx('text-lz-ink-3 hover:text-lz-ink', MOTION, FOCUS)}
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-4 overflow-y-auto px-4 py-3">
          <p className={cx('flex items-center gap-1.5', TYPE.bodyMuted)}>
            <Wand2 className="h-3.5 w-3.5 shrink-0" /> Answer a few questions and Goose saves a
            recipe your agent can run in a loop.
          </p>

          <Field
            label="What should your agent do, every run?"
            hint="The task it repeats each iteration — this becomes the recipe instructions."
          >
            <textarea
              value={instructions}
              onChange={(e) => setInstructions(e.target.value)}
              rows={5}
              placeholder="e.g. Check the CI dashboard for failed jobs, open the logs, and post a summary of any new failures to #alerts."
              className={cx(FIELD, 'resize-y')}
              autoFocus
            />
          </Field>

          <Field label="Short title" hint="A name for this recipe.">
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="e.g. CI failure watcher"
              className={FIELD}
            />
          </Field>

          <Field label="One-line description" hint="Optional — defaults to the title.">
            <input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="e.g. Watches CI and summarizes new failures."
              className={FIELD}
            />
          </Field>

          {error && (
            <div
              className={cx(
                'border border-lz-err px-3 py-2 text-lz-meta',
                TONE_TEXT.err,
                RADIUS.control
              )}
            >
              {error}
            </div>
          )}
        </div>

        <div className="flex items-center justify-between border-t border-lz-border px-4 py-3">
          <span className="text-lz-meta text-lz-ink-3">
            Saved to Recipes — then create a loop from it.
          </span>
          <Button
            variant="primary"
            size="sm"
            onClick={() => void save()}
            disabled={!canSave || saving}
            icon={saving ? <Loader2 className="animate-spin" /> : <Check />}
          >
            Save recipe
          </Button>
        </div>
      </div>
    </div>
  );
}

export default RecipeWizard;
