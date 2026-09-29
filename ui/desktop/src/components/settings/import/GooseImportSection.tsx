import React, { useState } from 'react';
import { FolderOpen, Check, X, Loader2, Repeat, FileWarning } from 'lucide-react';
import { toast } from 'react-toastify';
import { Switch } from '../../ui/switch';
import { parseRecipeFromFile } from '../../../recipe';
import { saveRecipe, listSavedRecipes } from '../../../recipe/recipe_management';
import { defineMessages, useIntl } from '../../../i18n';

/**
 * Goose Local Edition — import goose's OWN recipes from ANOTHER goose setup. Pick a source goose
 * config/data folder; its recipes/ (*.yaml|json) are saved via saveRecipe.
 *
 * The scheduler's recipe loop (`loop_config` on a schedule.json job) was retired (Q-227/Q-228, L8):
 * loops now live in the chat (session loops). A source schedule.json is still scanned so every job
 * that carries a `loop_config` is NAMED as not imported rather than silently dropped, and a
 * schedule.json that cannot be read is named with its error rather than read as "no loops".
 */

const i18n = defineMessages({
  title: {
    id: 'gooseImportSection.title',
    defaultMessage: 'From another Goose',
  },
  chooseSource: {
    id: 'gooseImportSection.chooseSource',
    defaultMessage: 'Choose source folder',
  },
  pickPrompt: {
    id: 'gooseImportSection.pickPrompt',
    defaultMessage: 'Pick another Goose config/data folder to import its recipes.',
  },
  scanning: {
    id: 'gooseImportSection.scanning',
    defaultMessage: 'Scanning {dir}…',
  },
  nothingFound: {
    id: 'gooseImportSection.nothingFound',
    defaultMessage: 'No recipes found under {dir}.',
  },
  recipesHeader: {
    id: 'gooseImportSection.recipesHeader',
    defaultMessage: 'Recipes ({count})',
  },
  importRecipes: {
    id: 'gooseImportSection.importRecipes',
    defaultMessage: 'Import {count, plural, one {# recipe} other {# recipes}}',
  },
  importedRecipes: {
    id: 'gooseImportSection.importedRecipes',
    defaultMessage: 'Imported {ok, plural, one {# recipe} other {# recipes}}',
  },
  importedRecipesWithFailures: {
    id: 'gooseImportSection.importedRecipesWithFailures',
    defaultMessage: 'Imported {ok, plural, one {# recipe} other {# recipes}}, {failed} failed',
  },
  loopsHeader: {
    id: 'gooseImportSection.loopsHeader',
    defaultMessage: 'Loops ({count})',
  },
  loopNotImported: {
    id: 'gooseImportSection.loopNotImported',
    defaultMessage: 'Loops are no longer imported — {id} ({cron}). Recipes still import.',
  },
  scheduleUnreadable: {
    id: 'gooseImportSection.scheduleUnreadable',
    defaultMessage: 'schedule.json could not be read: {error}',
  },
  scheduleNotAList: {
    id: 'gooseImportSection.scheduleNotAList',
    defaultMessage: 'schedule.json could not be read: it is not a list of schedules',
  },
});

const AZURE = '#2e8bff';
type ItemStatus = 'importing' | 'done' | 'error';

interface RecipeScan {
  file: string;
  name: string;
}
interface RetiredLoop {
  id: string;
  cron: string;
}
type ScheduleProblem = { kind: 'unreadable'; error: string } | { kind: 'notAList' };

const STATUS_ICON: Record<ItemStatus, React.ReactNode> = {
  importing: <Loader2 className="h-4 w-4 animate-spin" style={{ color: AZURE }} />,
  done: <Check className="h-4 w-4" style={{ color: '#2ecc71' }} strokeWidth={3} />,
  error: <X className="h-4 w-4" style={{ color: '#ff3b30' }} strokeWidth={3} />,
};

const RECIPE_RE = /\.(ya?ml|json)$/i;
const ABSENT_FILE_RE = /no such file/i;

/** Every job in a source schedule.json that carries the retired `loop_config`, or the named reason the
 *  file could not be read. An absent file is neither: there is simply nothing to report. */
async function scanRetiredLoops(
  dir: string
): Promise<{ loops: RetiredLoop[]; problem: ScheduleProblem | null }> {
  // schedule.json must be inside the chosen folder (no `..` traversal).
  const res = await window.electron.readFile(`${dir}/schedule.json`);
  if (!res.found) {
    if (res.error && !ABSENT_FILE_RE.test(res.error)) {
      return { loops: [], problem: { kind: 'unreadable', error: res.error.trim() } };
    }
    return { loops: [], problem: null };
  }
  if (!res.file.trim()) return { loops: [], problem: null };
  let parsed: unknown;
  try {
    parsed = JSON.parse(res.file);
  } catch (e) {
    return {
      loops: [],
      problem: { kind: 'unreadable', error: e instanceof Error ? e.message : String(e) },
    };
  }
  if (!Array.isArray(parsed)) return { loops: [], problem: { kind: 'notAList' } };
  const loops: RetiredLoop[] = [];
  for (const job of parsed as Array<Record<string, unknown>>) {
    if (job && typeof job === 'object' && job.loop_config && typeof job.loop_config === 'object') {
      loops.push({ id: String(job.id ?? ''), cron: String(job.cron ?? '') });
    }
  }
  return { loops, problem: null };
}

// Module scope on purpose (Q-512): declared inside GooseImportSection it was a new component type on
// every render, so every row remounted when one switch toggled and the switch under the pointer was
// replaced by a new one.
function Row({
  label,
  checked,
  onToggle,
  status,
}: {
  label: string;
  checked: boolean;
  onToggle: () => void;
  status?: ItemStatus;
}) {
  return (
    <div className="flex items-center gap-3 py-2">
      <div className="min-w-0 flex-1">
        <div className="text-sm text-text-primary truncate font-mono">{label}</div>
      </div>
      <div className="w-6 flex justify-center shrink-0">{status ? STATUS_ICON[status] : null}</div>
      <Switch checked={checked} onCheckedChange={onToggle} variant="mono" />
    </div>
  );
}

export default function GooseImportSection() {
  const intl = useIntl();
  const [sourceDir, setSourceDir] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);
  const [recipes, setRecipes] = useState<RecipeScan[]>([]);
  const [retiredLoops, setRetiredLoops] = useState<RetiredLoop[]>([]);
  const [scheduleProblem, setScheduleProblem] = useState<ScheduleProblem | null>(null);
  const [selRecipes, setSelRecipes] = useState<Set<string>>(new Set());
  const [results, setResults] = useState<Record<string, ItemStatus>>({});
  const [busy, setBusy] = useState(false);

  const chooseAndScan = async () => {
    const res = await window.electron.directoryChooser();
    if (res.canceled || res.filePaths.length === 0) return;
    const dir = res.filePaths[0];
    setSourceDir(dir);
    setScanning(true);
    setResults({});
    try {
      const recipeFiles = (await window.electron.listFiles(`${dir}/recipes`).catch(() => []))
        .filter((f) => RECIPE_RE.test(f))
        .map((f) => ({ file: f, name: f.replace(RECIPE_RE, '') }));
      setRecipes(recipeFiles);
      setSelRecipes(new Set(recipeFiles.map((r) => r.file)));

      const scan = await scanRetiredLoops(dir);
      setRetiredLoops(scan.loops);
      setScheduleProblem(scan.problem);
    } finally {
      setScanning(false);
    }
  };

  const toggleRecipe = (file: string) =>
    setSelRecipes((prev) => {
      const next = new Set(prev);
      if (next.has(file)) next.delete(file);
      else next.add(file);
      return next;
    });

  const importRecipes = async () => {
    if (!sourceDir) return;
    setBusy(true);
    let ok = 0;
    let failed = 0;
    // Idempotent: overwrite a same-titled recipe rather than creating a duplicate file each click.
    const existing = await listSavedRecipes().catch(() => []);
    const idByTitle = new Map(existing.map((e) => [e.recipe?.title, e.id] as const));
    for (const r of recipes.filter((x) => selRecipes.has(x.file))) {
      const rk = `recipe:${r.file}`;
      setResults((s) => ({ ...s, [rk]: 'importing' }));
      try {
        const content = await window.electron.readFile(`${sourceDir}/recipes/${r.file}`);
        if (!content.found || !content.file) throw new Error('unreadable');
        const recipe = await parseRecipeFromFile(content.file);
        await saveRecipe(recipe, idByTitle.get(recipe.title) ?? null);
        ok += 1;
        setResults((s) => ({ ...s, [rk]: 'done' }));
      } catch {
        failed += 1;
        setResults((s) => ({ ...s, [rk]: 'error' }));
      }
    }
    setBusy(false);
    if (failed) {
      toast.error(intl.formatMessage(i18n.importedRecipesWithFailures, { ok, failed }));
    } else {
      toast.success(intl.formatMessage(i18n.importedRecipes, { ok }));
    }
  };

  const scheduleProblemText =
    scheduleProblem === null
      ? null
      : scheduleProblem.kind === 'unreadable'
        ? intl.formatMessage(i18n.scheduleUnreadable, { error: scheduleProblem.error })
        : intl.formatMessage(i18n.scheduleNotAList);

  return (
    <div
      className="border border-border-primary"
      style={{ borderRadius: 3 }}
      data-testid="goose-import-section"
    >
      <div className="flex items-center justify-between px-3 py-2 bg-background-secondary border-b border-border-primary">
        <span className="text-sm font-semibold text-text-primary">
          {intl.formatMessage(i18n.title)}
        </span>
        <button
          onClick={() => void chooseAndScan()}
          disabled={scanning}
          className="flex items-center gap-1 text-xs border border-border-primary px-2 py-1 text-text-primary hover:border-text-secondary transition-colors disabled:cursor-not-allowed disabled:border-lz-border disabled:bg-lz-surface-2 disabled:text-lz-ink-3"
          style={{ borderRadius: 3 }}
        >
          <FolderOpen className="h-3.5 w-3.5" /> {intl.formatMessage(i18n.chooseSource)}
        </button>
      </div>

      {!sourceDir ? (
        <div className="px-3 py-4 text-xs text-text-secondary">
          {intl.formatMessage(i18n.pickPrompt)}
        </div>
      ) : scanning ? (
        <div className="px-3 py-4 text-xs text-text-secondary">
          {intl.formatMessage(i18n.scanning, { dir: sourceDir })}
        </div>
      ) : recipes.length === 0 && retiredLoops.length === 0 && scheduleProblemText === null ? (
        <div className="px-3 py-4 text-xs text-text-secondary">
          {intl.formatMessage(i18n.nothingFound, { dir: sourceDir })}
        </div>
      ) : (
        <div className="px-3 py-2 space-y-3">
          {recipes.length > 0 && (
            <div>
              <div className="flex items-center gap-1.5 text-xs font-semibold text-text-primary mb-1">
                <FolderOpen className="h-3.5 w-3.5" />{' '}
                {intl.formatMessage(i18n.recipesHeader, { count: recipes.length })}
              </div>
              <div className="divide-y divide-border-primary">
                {recipes.map((r) => (
                  <Row
                    key={r.file}
                    label={r.name}
                    checked={selRecipes.has(r.file)}
                    onToggle={() => toggleRecipe(r.file)}
                    status={results[`recipe:${r.file}`]}
                  />
                ))}
              </div>
              <div className="flex justify-end pt-1">
                <button
                  onClick={() => void importRecipes()}
                  disabled={busy || selRecipes.size === 0}
                  className="text-xs font-semibold px-3 py-1.5 bg-lz-accent text-lz-accent-ink hover:bg-lz-accent-hover disabled:cursor-not-allowed disabled:bg-lz-surface-2 disabled:text-lz-ink-3"
                  style={{ borderRadius: 3 }}
                >
                  {intl.formatMessage(i18n.importRecipes, { count: selRecipes.size })}
                </button>
              </div>
            </div>
          )}

          {retiredLoops.length > 0 && (
            <div data-testid="goose-import-retired-loops">
              <div className="flex items-center gap-1.5 text-xs font-semibold text-text-primary mb-1">
                <Repeat className="h-3.5 w-3.5" />{' '}
                {intl.formatMessage(i18n.loopsHeader, { count: retiredLoops.length })}
              </div>
              <div className="divide-y divide-border-primary">
                {retiredLoops.map((l, index) => (
                  <div
                    key={`${l.id}:${index}`}
                    className="py-2 text-xs text-text-primary"
                    data-testid="goose-import-retired-loop"
                  >
                    {intl.formatMessage(i18n.loopNotImported, { id: l.id, cron: l.cron })}
                  </div>
                ))}
              </div>
            </div>
          )}

          {scheduleProblemText !== null && (
            <div
              className="flex items-start gap-1.5 py-2 text-xs text-text-primary"
              data-testid="goose-import-schedule-problem"
            >
              <FileWarning className="h-3.5 w-3.5 shrink-0" style={{ color: '#ff3b30' }} />
              <span>{scheduleProblemText}</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
