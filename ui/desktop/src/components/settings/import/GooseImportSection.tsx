import React, { useState } from 'react';
import { FolderOpen, Check, X, Loader2, Repeat } from 'lucide-react';
import { toast } from 'react-toastify';
import { Switch } from '../../ui/switch';
import { parseRecipeFromFile } from '../../../recipe';
import { saveRecipe, listSavedRecipes } from '../../../recipe/recipe_management';
import { acpCreateSchedule } from '../../../acp/schedules';

/**
 * Goose Local Edition — import goose's OWN recipes + loops from ANOTHER goose setup (the "loops of its own"
 * ask). Pick a source goose config/data folder; it is scanned for recipes/ (*.yaml|json) and a schedule.json
 * whose entries with loop_config are loops. Recipes are saved via saveRecipe; loops are recreated via
 * acpCreateSchedule after mapping the on-disk snake_case (stop_check:{Shell:{command}}) to the wire DTO.
 *
 * CONFIDENCE: recipes = HIGH (reuses the tested parseRecipeFromFile + saveRecipe). Loops = MEDIUM — the
 * schedule.json format mapping is tested on synthetic data only (no real loops existed locally to test).
 */

const AZURE = '#2e8bff';
type ItemStatus = 'idle' | 'importing' | 'done' | 'skipped' | 'error';

interface RecipeScan {
  file: string;
  name: string;
}
interface OnDiskLoopConfig {
  max_iterations?: number;
  // serde serializes SuccessCheck internally-tagged: {"type":"Shell","command":"..."} — NOT {Shell:{...}}.
  stop_check?: { type?: string; command?: string } | null;
  state_artifact?: string | null;
}
interface LoopScan {
  id: string;
  cron: string;
  loop: OnDiskLoopConfig;
}

const STATUS_ICON: Record<ItemStatus, React.ReactNode> = {
  idle: null,
  importing: <Loader2 className="h-4 w-4 animate-spin" style={{ color: AZURE }} />,
  done: <Check className="h-4 w-4" style={{ color: '#2ecc71' }} strokeWidth={3} />,
  skipped: <span className="text-[10px] text-text-secondary">present</span>,
  error: <X className="h-4 w-4" style={{ color: '#ff3b30' }} strokeWidth={3} />,
};

const RECIPE_RE = /\.(ya?ml|json)$/i;

async function firstReadable(paths: string[]): Promise<string | null> {
  for (const p of paths) {
    const r = await window.electron.readFile(p);
    if (r.found && r.file) return r.file;
  }
  return null;
}

export default function GooseImportSection() {
  const [sourceDir, setSourceDir] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);
  const [recipes, setRecipes] = useState<RecipeScan[]>([]);
  const [loops, setLoops] = useState<LoopScan[]>([]);
  const [selRecipes, setSelRecipes] = useState<Set<string>>(new Set());
  const [selLoops, setSelLoops] = useState<Set<string>>(new Set());
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

      // schedule.json must be inside the chosen folder (no `..` traversal), so scheduled_recipes/ resolves
      // consistently under the same dir at import time.
      const scheduleRaw = await firstReadable([`${dir}/schedule.json`]);
      const parsedLoops: LoopScan[] = [];
      if (scheduleRaw) {
        try {
          const jobs = JSON.parse(scheduleRaw) as Array<Record<string, unknown>>;
          for (const j of Array.isArray(jobs) ? jobs : []) {
            if (j.loop_config && typeof j.loop_config === 'object') {
              parsedLoops.push({
                id: String(j.id ?? ''),
                cron: String(j.cron ?? ''),
                loop: j.loop_config as OnDiskLoopConfig,
              });
            }
          }
        } catch {
          /* malformed schedule.json — no loops */
        }
      }
      setLoops(parsedLoops);
      setSelLoops(new Set(parsedLoops.map((l) => l.id)));
    } finally {
      setScanning(false);
    }
  };

  const toggle = (set: Set<string>, setter: (s: Set<string>) => void, key: string) => {
    const next = new Set(set);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    setter(next);
  };

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
    const msg = `Imported ${ok} recipe${ok === 1 ? '' : 's'}${failed ? `, ${failed} failed` : ''}`;
    if (failed) toast.error(msg);
    else toast.success(msg);
  };

  const importLoops = async () => {
    if (!sourceDir) return;
    setBusy(true);
    let ok = 0;
    let failed = 0;
    let skipped = 0;
    for (const l of loops.filter((x) => selLoops.has(x.id))) {
      const rk = `loop:${l.id}`;
      setResults((s) => ({ ...s, [rk]: 'importing' }));
      try {
        // The loop's recipe lives beside it as scheduled_recipes/<id>.{yaml,json}.
        const recipeRaw = await firstReadable([
          `${sourceDir}/scheduled_recipes/${l.id}.yaml`,
          `${sourceDir}/scheduled_recipes/${l.id}.yml`,
          `${sourceDir}/scheduled_recipes/${l.id}.json`,
        ]);
        if (!recipeRaw) throw new Error('recipe not found for loop');
        const recipe = await parseRecipeFromFile(recipeRaw);
        await acpCreateSchedule({
          id: l.id,
          recipe,
          cron: l.cron,
          loop_config: {
            maxIterations: l.loop.max_iterations ?? 1,
            stopCheckCommand: l.loop.stop_check?.command,
            stateArtifact: l.loop.state_artifact ?? undefined,
          },
        });
        ok += 1;
        setResults((s) => ({ ...s, [rk]: 'done' }));
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        if (/exist|already/i.test(msg)) {
          skipped += 1;
          setResults((s) => ({ ...s, [rk]: 'skipped' }));
        } else {
          failed += 1;
          setResults((s) => ({ ...s, [rk]: 'error' }));
        }
      }
    }
    setBusy(false);
    const msg = `Imported ${ok} loop${ok === 1 ? '' : 's'}${skipped ? `, ${skipped} already present` : ''}${failed ? `, ${failed} failed` : ''}`;
    if (failed) toast.error(msg);
    else toast.success(msg);
  };

  const Row = ({
    label,
    sub,
    checked,
    onToggle,
    status,
  }: {
    label: string;
    sub?: string;
    checked: boolean;
    onToggle: () => void;
    status?: ItemStatus;
  }) => (
    <div className="flex items-center gap-3 py-2">
      <div className="min-w-0 flex-1">
        <div className="text-sm text-text-primary truncate font-mono">{label}</div>
        {sub && <div className="text-xs text-text-secondary truncate">{sub}</div>}
      </div>
      <div className="w-6 flex justify-center shrink-0">{status ? STATUS_ICON[status] : null}</div>
      <Switch checked={checked} onCheckedChange={onToggle} variant="mono" />
    </div>
  );

  return (
    <div
      className="border border-border-primary"
      style={{ borderRadius: 3 }}
      data-testid="goose-import-section"
    >
      <div className="flex items-center justify-between px-3 py-2 bg-background-secondary border-b border-border-primary">
        <span className="text-sm font-semibold text-text-primary">From another Goose</span>
        <button
          onClick={() => void chooseAndScan()}
          disabled={scanning}
          className="flex items-center gap-1 text-xs border border-border-primary px-2 py-1 text-text-primary hover:border-text-secondary transition-colors disabled:opacity-50"
          style={{ borderRadius: 3 }}
        >
          <FolderOpen className="h-3.5 w-3.5" /> Choose source folder
        </button>
      </div>

      {!sourceDir ? (
        <div className="px-3 py-4 text-xs text-text-secondary">
          Pick another Goose config/data folder to import its recipes and loops.
        </div>
      ) : scanning ? (
        <div className="px-3 py-4 text-xs text-text-secondary">Scanning {sourceDir}…</div>
      ) : recipes.length === 0 && loops.length === 0 ? (
        <div className="px-3 py-4 text-xs text-text-secondary">
          No recipes or loops found under {sourceDir}.
        </div>
      ) : (
        <div className="px-3 py-2 space-y-3">
          {recipes.length > 0 && (
            <div>
              <div className="flex items-center gap-1.5 text-xs font-semibold text-text-primary mb-1">
                <FolderOpen className="h-3.5 w-3.5" /> Recipes ({recipes.length})
              </div>
              <div className="divide-y divide-border-primary">
                {recipes.map((r) => (
                  <Row
                    key={r.file}
                    label={r.name}
                    checked={selRecipes.has(r.file)}
                    onToggle={() => toggle(selRecipes, setSelRecipes, r.file)}
                    status={results[`recipe:${r.file}`]}
                  />
                ))}
              </div>
              <div className="flex justify-end pt-1">
                <button
                  onClick={() => void importRecipes()}
                  disabled={busy || selRecipes.size === 0}
                  className="text-xs font-semibold px-3 py-1.5 text-white disabled:opacity-50"
                  style={{ backgroundColor: AZURE, borderRadius: 3 }}
                >
                  Import {selRecipes.size} recipe{selRecipes.size === 1 ? '' : 's'}
                </button>
              </div>
            </div>
          )}

          {loops.length > 0 && (
            <div>
              <div className="flex items-center gap-1.5 text-xs font-semibold text-text-primary mb-1">
                <Repeat className="h-3.5 w-3.5" /> Loops ({loops.length})
              </div>
              <div className="divide-y divide-border-primary">
                {loops.map((l) => (
                  <Row
                    key={l.id}
                    label={l.id}
                    sub={`${l.cron} · ${l.loop.max_iterations ?? 1} iterations`}
                    checked={selLoops.has(l.id)}
                    onToggle={() => toggle(selLoops, setSelLoops, l.id)}
                    status={results[`loop:${l.id}`]}
                  />
                ))}
              </div>
              <div className="flex justify-end pt-1">
                <button
                  onClick={() => void importLoops()}
                  disabled={busy || selLoops.size === 0}
                  className="text-xs font-semibold px-3 py-1.5 text-white disabled:opacity-50"
                  style={{ backgroundColor: AZURE, borderRadius: 3 }}
                >
                  Import {selLoops.size} loop{selLoops.size === 1 ? '' : 's'}
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
