import React, { useCallback, useEffect, useState } from 'react';
import { RefreshCw, Check, X, Loader2, FileWarning } from 'lucide-react';
import { toast } from 'react-toastify';
import { LeanZero } from '../../icons';
import { Switch } from '../../ui/switch';
import { createSkillSource } from '../../../acp/sources';
import { addConfigExtension } from '../../../acp/extensions';
import { acpUpsertConfig, acpReadConfig } from '../../../acp/config';
import type { ExtensionConfig } from '../../../types/extensions';
import { getInitialWorkingDir } from '../../../utils/workingDir';
import GooseImportSection from './GooseImportSection';

/**
 * Goose Local Edition — Import hub. Scans a source (Claude Code today) for artifacts and lets the user
 * conditionally import the categories that exist. Phase 1: Skills (~/.claude/skills). Later phases add MCP
 * servers, memory (CLAUDE.md), and goose-config recipes/loops as additional sections. Sharp full-border
 * cards, solid azure accents, custom Switch rows (no native controls / left rail).
 */

const AZURE = '#2e8bff';
const CLAUDE_SKILLS_DIR = '~/.claude/skills';
const CLAUDE_JSON = '~/.claude.json';
const CLAUDE_MD = '~/.claude/CLAUDE.md';

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
  const src = text.replace(/^\uFEFF/, '').replace(/^\s*\n/, ''); // strip BOM + leading blank lines
  const m = src.match(/^---\s*\n([\s\S]*?)\n---\s*\n?([\s\S]*)$/);
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

/** True when an import error means "the item already exists". ACP JSON-RPC errors carry that text in
 *  `.data` (a string), NOT `.message` (which is the generic "Invalid params"), so check both — otherwise a
 *  duplicate body-only skill is mis-reported as "failed" instead of "already present". */
function isAlreadyExists(e: unknown): boolean {
  const rec = e as { message?: unknown; data?: unknown };
  const dataStr = typeof rec?.data === 'string' ? rec.data : JSON.stringify(rec?.data ?? '');
  const msg = e instanceof Error ? e.message : String(e);
  return /already exists|\bexists\b/i.test(`${msg} ${dataStr}`);
}

/** Goose skill names must match ^[a-z0-9-]+$, <=64 chars, no leading/trailing hyphen. */
function slugifySkillName(name: string): string {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9-]+/g, '-')
      .replace(/-+/g, '-')
      .slice(0, 64)
      .replace(/^-|-$/g, '') || 'skill' // strip AFTER slice so truncation can't leave a dangling hyphen
  );
}

async function scanClaudeSkills(): Promise<ClaudeSkill[]> {
  const names = await window.electron.listFiles(CLAUDE_SKILLS_DIR).catch(() => [] as string[]);
  const skills: ClaudeSkill[] = [];
  const usedSlugs = new Set<string>();
  for (const dirName of names) {
    const res = await window.electron.readFile(`${CLAUDE_SKILLS_DIR}/${dirName}/SKILL.md`);
    if (!res.found || !res.file) continue; // not a skill dir
    const { fm, body } = parseFrontmatter(res.file);
    const entries = await window.electron
      .listFiles(`${CLAUDE_SKILLS_DIR}/${dirName}`)
      .catch(() => [] as string[]);
    const supportingCount = entries.filter((e) => e !== 'SKILL.md').length;
    const displayName = fm.name || dirName;
    // Disambiguate so two Claude dirs that slugify the same never clobber each other on import.
    const base = slugifySkillName(displayName);
    let slug = base;
    let n = 2;
    while (usedSlugs.has(slug)) {
      const sfx = `-${n++}`;
      slug = base.slice(0, 64 - sfx.length) + sfx;
    }
    usedSlugs.add(slug);
    skills.push({
      dirName,
      name: slug,
      displayName,
      description: fm.description || '',
      body: body.trim(),
      supportingCount,
    });
  }
  return skills.sort((a, b) => a.displayName.localeCompare(b.displayName));
}

interface McpServerScan {
  name: string;
  transport: 'stdio' | 'http' | 'sse';
  command?: string;
  args: string[];
  url?: string;
  headers: Record<string, string>;
  envValues: Record<string, string>; // env var name -> value (stored as secrets on import)
  supported: boolean; // goose dropped the sse transport
}

/** Read ~/.claude.json and map its `mcpServers` block into a normalized scan list. Claude's shape:
 *  `{ "<name>": { type?: 'stdio'|'http'|'sse', command?, args?, env?, url?, headers? } }`. `type` may be
 *  absent (command => stdio, url => http). `env`/`headers` may be an object, or `[]` when empty. */
async function scanClaudeMcp(): Promise<{ servers: McpServerScan[]; parseError: boolean }> {
  const res = await window.electron.readFile(CLAUDE_JSON);
  if (!res.found || !res.file) return { servers: [], parseError: false };
  let json: unknown;
  try {
    json = JSON.parse(res.file);
  } catch {
    return { servers: [], parseError: true }; // present but malformed — surface it, don't look "absent"
  }
  const servers = (json as { mcpServers?: Record<string, Record<string, unknown>> })?.mcpServers;
  if (!servers || typeof servers !== 'object') return { servers: [], parseError: false };
  const asRecord = (v: unknown): Record<string, string> =>
    v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, string>) : {};
  const list = Object.entries(servers).map(([name, s]) => {
    const declared = String(s.type ?? '').toLowerCase();
    const command = typeof s.command === 'string' ? s.command : undefined;
    const url = typeof s.url === 'string' ? s.url : undefined;
    const transport: McpServerScan['transport'] =
      declared === 'sse'
        ? 'sse'
        : declared === 'http' || (url && declared !== 'stdio')
          ? 'http'
          : 'stdio';
    // Not importable unless the transport's mandatory field is present (stdio needs command, http needs url).
    const usable = transport === 'stdio' ? Boolean(command) : transport === 'http' ? Boolean(url) : false;
    return {
      name,
      transport,
      command,
      args: Array.isArray(s.args) ? (s.args as unknown[]).map(String) : [],
      url,
      headers: asRecord(s.headers),
      envValues: asRecord(s.env),
      supported: transport !== 'sse' && usable,
    };
  });
  return { servers: list, parseError: false };
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
  const [mcpServers, setMcpServers] = useState<McpServerScan[]>([]);
  const [mcpParseError, setMcpParseError] = useState(false);
  const [selectedMcp, setSelectedMcp] = useState<Set<string>>(new Set());
  const [memory, setMemory] = useState<{ present: boolean; bytes: number }>({
    present: false,
    bytes: 0,
  });

  const rescan = useCallback(async () => {
    setLoading(true);
    try {
      const [found, mcp, mem] = await Promise.all([
        scanClaudeSkills(),
        scanClaudeMcp(),
        window.electron.readFile(CLAUDE_MD),
      ]);
      setSkills(found);
      setSelected(new Set(found.map((s) => s.dirName))); // default: all selected
      setMcpServers(mcp.servers);
      setMcpParseError(mcp.parseError);
      setSelectedMcp(new Set(mcp.servers.filter((m) => m.supported).map((m) => m.name)));
      setMemory({ present: !!mem.found, bytes: mem.found && mem.file ? mem.file.length : 0 });
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
        const alreadyExists = isAlreadyExists(e);
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

  const toggleMcp = (name: string) => {
    setSelectedMcp((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const importSelectedMcp = async () => {
    setImporting(true);
    let ok = 0;
    let failed = 0;
    const chosen = mcpServers.filter((m) => selectedMcp.has(m.name) && m.supported);
    for (const srv of chosen) {
      const rk = `mcp:${srv.name}`;
      setResults((r) => ({ ...r, [rk]: { status: 'importing' } }));
      try {
        const envKeys = Object.keys(srv.envValues).filter((k) => srv.envValues[k]);
        // The extension mapping drops env VALUES, so store each as a secret and list its key in env_keys.
        // Env keys live in the GLOBAL config namespace, so a server env like ANTHROPIC_API_KEY would
        // clobber your provider key. Never overwrite an existing key — keep the current value and just
        // reference it in env_keys.
        for (const k of envKeys) {
          const existing = await acpReadConfig(k, true).catch(() => null);
          if (existing == null) {
            await acpUpsertConfig(k, srv.envValues[k], true);
          }
        }
        const config: ExtensionConfig =
          srv.transport === 'http'
            ? {
                type: 'streamable_http',
                name: srv.name,
                uri: srv.url ?? '',
                headers: srv.headers,
                ...(envKeys.length ? { env_keys: envKeys } : {}),
              }
            : {
                type: 'stdio',
                name: srv.name,
                cmd: srv.command ?? '',
                args: srv.args,
                ...(envKeys.length ? { env_keys: envKeys } : {}),
              };
        await addConfigExtension(config, true);
        ok += 1;
        setResults((r) => ({ ...r, [rk]: { status: 'done' } }));
      } catch (e) {
        failed += 1;
        setResults((r) => ({
          ...r,
          [rk]: { status: 'error', message: e instanceof Error ? e.message : String(e) },
        }));
      }
    }
    setImporting(false);
    const msg = `Added ${ok} MCP server${ok === 1 ? '' : 's'}${failed ? `, ${failed} failed` : ''}`;
    if (failed) toast.error(msg);
    else toast.success(msg);
  };

  const importMemory = async () => {
    setResults((r) => ({ ...r, memory: { status: 'importing' } }));
    setImporting(true);
    try {
      // Shells `goose import claude-code --memory --apply --yes` — idempotent, provenance-fenced;
      // brings CLAUDE.md into ~/.config/goose/.goosehints + auto-memory notes into memory/.
      const out = await window.electron.importClaudeCode(['--memory']);
      if (!out.ok) throw new Error(out.stderr || out.error || 'import failed');
      setResults((r) => ({ ...r, memory: { status: 'done' } }));
      toast.success('Imported CLAUDE.md into goose hints + memory');
    } catch (e) {
      setResults((r) => ({
        ...r,
        memory: { status: 'error', message: e instanceof Error ? e.message : String(e) },
      }));
      toast.error('Memory import failed');
    } finally {
      setImporting(false);
    }
  };

  const selectedCount = skills.filter((s) => selected.has(s.dirName)).length;
  const selectedMcpCount = mcpServers.filter((m) => selectedMcp.has(m.name) && m.supported).length;

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
          disabled={loading || importing}
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
      ) : skills.length === 0 && mcpServers.length === 0 && !memory.present && !mcpParseError ? (
        <div
          className="text-sm text-text-secondary border border-border-primary px-3 py-4 text-center"
          style={{ borderRadius: 3 }}
        >
          Nothing found in ~/.claude to import.
        </div>
      ) : (
        <>
        {mcpParseError && (
          <div
            className="flex items-center gap-2 text-xs px-3 py-2 border border-border-primary"
            style={{ color: '#f5a623', borderRadius: 3 }}
          >
            <FileWarning className="h-4 w-4 shrink-0" />
            ~/.claude.json is present but couldn't be parsed — MCP servers may be hidden. Fix the JSON and
            Rescan.
          </div>
        )}
        {skills.length > 0 && (
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

        {mcpServers.length > 0 && (
          <SectionCard title="MCP servers" count={mcpServers.length}>
            <div className="px-3 py-1 divide-y divide-border-primary">
              {mcpServers.map((srv) => {
                const isSel = selectedMcp.has(srv.name) && srv.supported;
                const res = results[`mcp:${srv.name}`];
                const detail =
                  srv.transport === 'http'
                    ? srv.url
                    : `${srv.command ?? ''} ${srv.args.join(' ')}`.trim();
                return (
                  <div key={srv.name} className="flex items-center gap-3 py-2">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm text-text-primary truncate">{srv.name}</span>
                        <span className="text-[10px] text-text-secondary shrink-0">{srv.transport}</span>
                        {!srv.supported && (
                          <span className="text-[10px] shrink-0" style={{ color: '#ff3b30' }}>
                            unsupported (sse)
                          </span>
                        )}
                        {Object.keys(srv.envValues).length > 0 && (
                          <span
                            className="text-[10px] text-text-secondary shrink-0"
                            title="env values are imported as secrets"
                          >
                            sets secrets
                          </span>
                        )}
                      </div>
                      {detail && (
                        <div className="text-xs text-text-secondary truncate font-mono">{detail}</div>
                      )}
                    </div>
                    <div className="w-6 flex justify-center shrink-0">
                      {res ? STATUS_ICON[res.status] : null}
                    </div>
                    <Switch
                      checked={isSel}
                      onCheckedChange={() => toggleMcp(srv.name)}
                      variant="mono"
                      disabled={!srv.supported}
                    />
                  </div>
                );
              })}
            </div>
            <div className="flex items-center justify-between px-3 py-2 border-t border-border-primary">
              <span className="text-xs text-text-secondary">{selectedMcpCount} selected</span>
              <button
                onClick={() => void importSelectedMcp()}
                disabled={importing || selectedMcpCount === 0}
                className="flex items-center gap-1.5 text-xs font-semibold px-3 py-1.5 text-white transition-opacity hover:opacity-90 disabled:opacity-50"
                style={{ backgroundColor: AZURE, borderRadius: 3 }}
              >
                {importing ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
                Add {selectedMcpCount} server{selectedMcpCount === 1 ? '' : 's'}
              </button>
            </div>
          </SectionCard>
        )}

        {memory.present && (
          <SectionCard title="Memory" count={1}>
            <div className="px-3 py-2 flex items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="text-sm text-text-primary">CLAUDE.md → goose hints + memory</div>
                <div className="text-xs text-text-secondary">
                  {(memory.bytes / 1024).toFixed(1)} KB of global instructions · idempotent, re-runnable
                </div>
              </div>
              <div className="flex items-center gap-3 shrink-0">
                <div className="w-6 flex justify-center">
                  {results.memory ? STATUS_ICON[results.memory.status] : null}
                </div>
                <button
                  onClick={() => void importMemory()}
                  disabled={importing}
                  className="flex items-center gap-1.5 text-xs font-semibold px-3 py-1.5 text-white transition-opacity hover:opacity-90 disabled:opacity-50"
                  style={{ backgroundColor: AZURE, borderRadius: 3 }}
                >
                  {importing ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
                  Import memory
                </button>
              </div>
            </div>
          </SectionCard>
        )}
        </>
      )}

      <div className="pt-2 border-t border-border-primary mt-4">
        <p className="text-xs text-text-secondary mb-2">
          Migrate your own Goose recipes and loops from another setup.
        </p>
        <GooseImportSection />
      </div>
    </div>
  );
}
