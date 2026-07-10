import { useState, type ReactNode } from 'react';
import { X, Wand2, Loader2, Check } from 'lucide-react';
import { toast } from 'react-toastify';
import { LeanZero } from '../icons';
import type { Recipe } from '../../recipe';
import { saveRecipe } from '../../recipe/recipe_management';

/**
 * Goose Local Edition — a GUIDED recipe wizard. It ASKS the questions in the UI (what should the agent do,
 * a name, a one-line description) and assembles + saves a real recipe. This replaces the earlier "build with
 * the swarm" step, which wrongly sent a "please ask me questions" prompt to the swarm PROVIDER — but the
 * swarm is a build orchestrator, not a conversational model, so it just started a build run instead of a Q&A.
 */

const AZURE = '#2e8bff';

const inputClass =
  'w-full bg-background-primary border border-border-primary px-2.5 py-1.5 text-sm text-text-primary focus:border-text-secondary outline-none';

// Defined at module scope on purpose: a component declared inside RecipeWizard would be a new type
// on every render, remounting its inputs and dropping focus after each keystroke.
function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div>
      <div className="text-sm text-text-primary mb-1">{label}</div>
      {hint && <div className="text-xs text-text-secondary mb-1.5">{hint}</div>}
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
        className="bg-background-primary border border-border-primary shadow-lg w-[560px] max-h-[88vh] flex flex-col"
        style={{ borderRadius: 3 }}
        data-testid="recipe-wizard"
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-border-primary">
          <div className="flex items-center gap-2">
            <span
              className="flex items-center justify-center h-6 w-6"
              style={{ backgroundColor: AZURE }}
            >
              <LeanZero className="h-4 w-4 text-white" />
            </span>
            <h3 className="text-sm font-semibold text-text-primary">Draft a recipe</h3>
          </div>
          <button onClick={onClose} className="text-text-secondary hover:text-text-primary" aria-label="Close">
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="px-4 py-3 overflow-y-auto space-y-4">
          <p className="text-xs text-text-secondary flex items-center gap-1.5">
            <Wand2 className="h-3.5 w-3.5" /> Answer a few questions and Goose saves a recipe your agent can
            run in a loop.
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
              className={inputClass}
              style={{ borderRadius: 3, resize: 'vertical' }}
              autoFocus
            />
          </Field>

          <Field label="Short title" hint="A name for this recipe.">
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="e.g. CI failure watcher"
              className={inputClass}
              style={{ borderRadius: 3 }}
            />
          </Field>

          <Field label="One-line description" hint="Optional — defaults to the title.">
            <input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="e.g. Watches CI and summarizes new failures."
              className={inputClass}
              style={{ borderRadius: 3 }}
            />
          </Field>

          {error && (
            <div
              className="text-xs px-3 py-2 border"
              style={{ color: '#ff3b30', borderColor: '#ff3b30', borderRadius: 3 }}
            >
              {error}
            </div>
          )}
        </div>

        <div className="px-4 py-3 border-t border-border-primary flex items-center justify-between">
          <span className="text-xs text-text-secondary">Saved to Recipes — then create a loop from it.</span>
          <button
            onClick={() => void save()}
            disabled={!canSave || saving}
            className="flex items-center gap-1.5 text-xs font-semibold px-3 py-1.5 text-white transition-opacity hover:opacity-90 disabled:opacity-50"
            style={{ backgroundColor: AZURE, borderRadius: 3 }}
          >
            {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Check className="h-3.5 w-3.5" />}
            Save recipe
          </button>
        </div>
      </div>
    </div>
  );
}

export default RecipeWizard;
