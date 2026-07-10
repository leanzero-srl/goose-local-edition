import React, { useCallback, useEffect, useState } from 'react';
import { RefreshCw, Check, X, Loader2, FileWarning } from 'lucide-react';
import { toast } from 'react-toastify';
import { LeanZero } from '../../icons';
import { Switch } from '../../ui/switch';
import { createSkillSource } from '../../../acp/sources';
import { getInitialWorkingDir } from '../../../utils/workingDir';

/**
 * Goose Local Edition — Import hub. Scans a source (Claude Code today) for artifacts and lets the user
 * conditionally import the categories that exist. Phase 1: Skills (~/.claude/skills). Later phases add MCP
 * servers, memory (CLAUDE.md), and goose-config recipes/loops as additional sections. Sharp full-border
 * cards, solid azure accents, custom Switch rows (no native controls / left rail).
 */

const AZURE = '#2e8bff';
const CLAUDE_SKILLS_DIR = '~/.claude/skills';

interface ClaudeSkill {
  dirName: string;
  name: string; // slugified, valid goose skill name
  displayName: string;
  description: string;
  body: string;
  supportingCount: number;
}

type ItemStatus = 'idle' | 'importing' | 'done' | 'skipped' | 'error';

interface ImportResult {
  status: ItemStatus;
  message?: string;
}

/** Split a `---\nyaml\n---\nbody` SKILL.md into TOP-LEVEL frontmatter fields + the body (there is no
 *  frontmatter parser in the renderer). Only name/description are needed. Handles YAML block scalars
 *  (`description: >-` / `|` folded/literal multiline) — several real skills use them, and a naive parser
 *  would capture the literal ">-". Nested keys (indented, e.g. under `metadata:`) are ignored. */
function parseFrontmatter(text: string): { fm: Record<string, string>; body: string } {
  const m = text.match(/^---\s*\n([\s\S]*?)\n---\s*\n?([\s\S]*)$/);
  if (!m) return { fm: {}, body: text };
  const lines = m[1].split('\n');
  const fm: Record<string, string> = {};
  for (let i = 0; i < lines.length; i++) {
    const km = lines[i].match(/^([A-Za-z0-9_-]+):\s*(.*)$/); // top-level key only (no leading indent)
    if (!km) continue;
    const key = km[1];
    const rawVal = km[2].trim();
    if (/^[|>][+-]?$/.test(rawVal)) {
      // Block scalar: gather following blank/indented lines until the next top-level key.
      const block: string[] = [];
      let j = i + 1;
      while (j < lines.length && (lines[j].trim() === '' || /^\s/.test(lines[j]))) {
        block.push(lines[j]);
        j += 1;
      }
      const indents = block.filter((l) => l.trim()).map((l) => (l.match(/^(\s*)/)?.[1].length ?? 0));
      const minIndent = indents.length ? Math.min(...indents) : 0;
      const dedented = block.map((l) => l.slice(minIndent));
      fm[key] =
        rawVal[0] === '>'
          ? dedented.join(' ').replace(/\s+/g, ' ').trim() // folded
          : dedented.join('\n').trim(); // literal
      i = j - 1;
    } else {
      fm[key] = rawVal.replace(/^['"]|['"]$/g, '').trim();
    }
  }
  return { fm, body: m[2] };
}

/** Goose skill names must match ^[a-z0-9-]+$, <=64 chars, no leading/trailing hyphen. */
function slugifySkillName(name: string): string {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9-]+/g, '-')
      .replace(/-+/g, '-')
      .replace(/^-|-$/g, '')
      .slice(0, 64) || 'skill'
  );
}

async function scanClaudeSkills(): Promise<ClaudeSkill[]> {
  const names = await window.electron.listFiles(CLAUDE_SKILLS_DIR).catch(() => [] as string[]);
  const skills: ClaudeSkill[] = [];
  for (const dirName of names) {
    const res = await window.electron.readFile(`${CLAUDE_SKILLS_DIR}/${dirName}/SKILL.md`);
    if (!res.found || !res.file) continue; // not a skill dir
    const { fm, body } = parseFrontmatter(res.file);
    const entries = await window.electron
      .listFiles(`${CLAUDE_SKILLS_DIR}/${dirName}`)
      .catch(() => [] as string[]);
    const supportingCount = entries.filter((e) => e !== 'SKILL.md').length;
    const displayName = fm.name || dirName;
    skills.push({
      dirName,
      name: slugifySkillName(displayName),
      displayName,
      description: fm.description || '',
      body: body.trim(),
      supportingCount,
    });
  }
  return skills.sort((a, b) => a.displayName.localeCompare(b.displayName));
}

function SectionCard({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: React.ReactNode;
}) {
  return (
    <div className="border border-border-primary" style={{ borderRadius: 3 }}>
      <div className="flex items-center justify-between px-3 py-2 bg-background-secondary border-b border-border-primary">
        <span className="text-sm font-semibold text-text-primary">{title}</span>
        <span className="text-xs text-text-secondary">{count} found</span>
      </div>
      {children}
    </div>
  );
}

const STATUS_ICON: Record<ItemStatus, React.ReactNode> = {
  idle: null,
  importing: <Loader2 className="h-4 w-4 animate-spin" style={{ color: AZURE }} />,
  done: <Check className="h-4 w-4" style={{ color: '#2ecc71' }} strokeWidth={3} />,
  skipped: <span className="text-xs text-text-secondary">skipped</span>,
  error: <X className="h-4 w-4" style={{ color: '#ff3b30' }} strokeWidth={3} />,
};

export default function ImportView() {
  const [loading, setLoading] = useState(true);
  const [skills, setSkills] = useState<ClaudeSkill[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [scope, setScope] = useState<'global' | 'project'>('global');
  const [results, setResults] = useState<Record<string, ImportResult>>({});
  const [importing, setImporting] = useState(false);

  const rescan = useCallback(async () => {
    setLoading(true);
    try {
      const found = await scanClaudeSkills();
      setSkills(found);
      setSelected(new Set(found.map((s) => s.dirName))); // default: all selected
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void rescan();
  }, [rescan]);

  const toggle = (dirName: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(dirName)) next.delete(dirName);
      else next.add(dirName);
      return next;
    });
  };

  const importSelectedSkills = async () => {
    const projectDir = getInitialWorkingDir();
    const target =
      scope === 'global'
        ? ({ scope: 'global' } as const)
        : ({ scope: 'projectDir', projectDir } as const);
    const destBase = scope === 'global' ? '~/.agents/skills' : `${projectDir}/.agents/skills`;

    setImporting(true);
    const chosen = skills.filter((s) => selected.has(s.dirName));
    let ok = 0;
    let failed = 0;
    let skipped = 0;
    for (const skill of chosen) {
      setResults((r) => ({ ...r, [skill.dirName]: { status: 'importing' } }));
      try {
        if (skill.supportingCount > 0) {
          // Preserve docs/scripts/templates by copying the directory (the create API drops them).
          const out = await window.electron.copyDir(
            `${CLAUDE_SKILLS_DIR}/${skill.dirName}`,
            `${destBase}/${skill.name}`
          );
          if (!out.ok) throw new Error(out.error || 'copy failed');
        } else {
          await createSkillSource({
            name: skill.name,
            description: skill.description,
            content: skill.body,
            target,
          });
        }
        ok += 1;
        setResults((r) => ({ ...r, [skill.dirName]: { status: 'done' } }));
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        const alreadyExists = /exist/i.test(msg);
        if (alreadyExists) skipped += 1;
        else failed += 1;
        setResults((r) => ({
          ...r,
          [skill.dirName]: { status: alreadyExists ? 'skipped' : 'error', message: msg },
        }));
      }
    }
    setImporting(false);
    const parts = [`Imported ${ok} skill${ok === 1 ? '' : 's'}`];
    if (skipped) parts.push(`${skipped} already present`);
    if (failed) parts.push(`${failed} failed`);
    const msg = parts.join(', ');
    if (failed) toast.error(msg);
    else toast.success(msg);
  };

  const selectedCount = skills.filter((s) => selected.has(s.dirName)).length;

  return (
    <div className="space-y-4 pr-4 pb-8 max-w-3xl">
      <div className="flex items-start justify-between gap-4">
        <div className="flex items-center gap-2">
          <span className="flex items-center justify-center h-6 w-6" style={{ backgroundColor: AZURE }}>
            <LeanZero className="h-4 w-4 text-white" />
          </span>
          <div>
            <h2 className="text-sm font-semibold text-text-primary">Import from Claude Code</h2>
            <p className="text-xs text-text-secondary">
              Bring your Claude Code skills, memory, and MCP servers into Goose Local Edition. Only what's
              present is shown.
            </p>
          </div>
        </div>
        <button
          onClick={() => void rescan()}
          disabled={loading}
          className="shrink-0 flex items-center gap-1 text-xs border border-border-primary px-2 py-1 text-text-primary hover:border-text-secondary transition-colors disabled:opacity-50"
          style={{ borderRadius: 3 }}
        >
          <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} /> Rescan
        </button>
      </div>

      {/* Scope */}
      <div className="flex items-center gap-2 text-xs">
        <span className="text-text-secondary">Install to:</span>
        <div className="inline-flex border border-border-primary" style={{ borderRadius: 3 }}>
          {(['global', 'project'] as const).map((s, i) => (
            <button
              key={s}
              onClick={() => setScope(s)}
              className={`px-2.5 py-0.5 ${
                scope === s ? 'font-semibold text-background-primary' : 'text-text-secondary'
              } ${i > 0 ? 'border-l border-border-primary' : ''}`}
              style={{ backgroundColor: scope === s ? AZURE : 'transparent' }}
            >
              {s === 'global' ? 'Global (~/.agents/skills)' : 'This project'}
            </button>
          ))}
        </div>
      </div>

      {loading ? (
        <div className="text-sm text-text-secondary">Scanning ~/.claude…</div>
      ) : skills.length === 0 ? (
        <div
          className="text-sm text-text-secondary border border-border-primary px-3 py-4 text-center"
          style={{ borderRadius: 3 }}
        >
          No Claude Code skills found in ~/.claude/skills.
        </div>
      ) : (
        <SectionCard title="Skills" count={skills.length}>
          <div className="px-3 py-1 divide-y divide-border-primary">
            {skills.map((skill) => {
              const isSel = selected.has(skill.dirName);
              const res = results[skill.dirName];
              return (
                <div key={skill.dirName} className="flex items-center gap-3 py-2">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-sm text-text-primary truncate">{skill.displayName}</span>
                      {skill.supportingCount > 0 && (
                        <span
                          className="text-[10px] text-text-secondary flex items-center gap-0.5 shrink-0"
                          title={`${skill.supportingCount} supporting files — copied as a directory`}
                        >
                          <FileWarning className="h-3 w-3" />
                          {skill.supportingCount} files
                        </span>
                      )}
                    </div>
                    {skill.description && (
                      <div className="text-xs text-text-secondary truncate">{skill.description}</div>
                    )}
                  </div>
                  <div className="w-6 flex justify-center shrink-0">{res ? STATUS_ICON[res.status] : null}</div>
                  <Switch checked={isSel} onCheckedChange={() => toggle(skill.dirName)} variant="mono" />
                </div>
              );
            })}
          </div>
          <div className="flex items-center justify-between px-3 py-2 border-t border-border-primary">
            <span className="text-xs text-text-secondary">{selectedCount} selected</span>
            <button
              onClick={() => void importSelectedSkills()}
              disabled={importing || selectedCount === 0}
              className="flex items-center gap-1.5 text-xs font-semibold px-3 py-1.5 text-white transition-opacity hover:opacity-90 disabled:opacity-50"
              style={{ backgroundColor: AZURE, borderRadius: 3 }}
            >
              {importing ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
              Import {selectedCount} skill{selectedCount === 1 ? '' : 's'}
            </button>
          </div>
        </SectionCard>
      )}
    </div>
  );
}
