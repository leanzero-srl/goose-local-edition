import * as Dialog from '@radix-ui/react-dialog';
import { useConfig } from '../ConfigContext';
import { useId, useMemo, useRef, useState } from 'react';
import { FolderOpen, Plus, X } from 'lucide-react';
import {
  Button,
  Checkbox,
  Chip,
  Combobox,
  Disclosure,
  Segmented,
  SURFACE,
  TYPE,
  WEIGHT,
  cx,
  type ComboboxOption,
  type SegmentedOption,
} from '../lz';
import { getFriendlyTitle, getSubtitle } from '../settings/extensions/subcomponents/ExtensionList';
import { manifestYaml } from './agentWorkModel';

const DAYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'] as const;
const LENSES = ['factual', 'duplication', 'voice'] as const;

const field =
  'h-8 w-full rounded-lz-control border border-lz-border-strong bg-lz-surface px-2 text-lz-body text-lz-ink';
const label = cx(TYPE.meta, WEIGHT.medium, 'mb-1 block');
const sectionTitle = TYPE.h2;

/** The cadences people actually pick, as the engine's own `<n>s|m|h` strings — "daily" is 24h. */
const CADENCE_PRESETS = [
  { value: '15m', label: '15m' },
  { value: '30m', label: '30m' },
  { value: '1h', label: '1h' },
  { value: '4h', label: '4h' },
  { value: '24h', label: 'daily' },
] as const;
type CadencePick = (typeof CADENCE_PRESETS)[number]['value'] | 'custom';

/** Every IANA zone this runtime knows, with its current UTC offset as the hint. Computed once per
 *  app session (the list is the runtime's, not ours); an engine without supportedValuesOf leaves
 *  the list empty and the field still takes free text, which the create step validates. */
let zoneCache: ComboboxOption[] | null = null;
function timezoneOptions(): ComboboxOption[] {
  if (zoneCache) return zoneCache;
  const supported = (Intl as unknown as { supportedValuesOf?: (key: 'timeZone') => string[] })
    .supportedValuesOf;
  const zones = typeof supported === 'function' ? supported('timeZone') : [];
  const now = new Date();
  zoneCache = zones.map((zone) => {
    let hint: string | undefined;
    try {
      hint = new Intl.DateTimeFormat('en', { timeZone: zone, timeZoneName: 'shortOffset' })
        .formatToParts(now)
        .find((p) => p.type === 'timeZoneName')?.value;
    } catch {
      hint = undefined;
    }
    return { value: zone, hint };
  });
  return zoneCache;
}

/**
 * Creates an agent: a directory with agent.yaml, CHARTER.md, SCRATCHPAD.md, DAILY-LOG.md and
 * PENDING.md. Every control is Studio-built (day chips, a Segmented for approval) — no native
 * select, no window.prompt. The YAML is previewed before it is written.
 */
export function NewAgentDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (dir: string) => void;
}) {
  const id = useId();
  const { extensionsList } = useConfig();
  const [extensions, setExtensions] = useState<string[]>([]);
  const [preview, setPreview] = useState(false);
  const firstRef = useRef<HTMLInputElement>(null);
  const [dir, setDir] = useState('');
  const [name, setName] = useState('');
  const [title, setTitle] = useState('');
  const [charter, setCharter] = useState('');
  const [timezone, setTimezone] = useState(
    Intl.DateTimeFormat().resolvedOptions().timeZone || 'Europe/Bucharest'
  );
  const [from, setFrom] = useState('09:00');
  const [to, setTo] = useState('18:00');
  const [days, setDays] = useState<string[]>(['mon', 'tue', 'wed', 'thu', 'fri']);
  const [always, setAlways] = useState(false);
  const [cadence, setCadence] = useState('30m');
  const [envFile, setEnvFile] = useState('');
  const [poll, setPoll] = useState('');
  const [guard, setGuard] = useState('');
  const surgeonSeq = useRef(0);
  const nextSurgeonKey = () => `surgeon-${surgeonSeq.current++}`;
  const [surgeons, setSurgeons] = useState<
    { key: string; name: string; brief: string; readOnly: boolean }[]
  >(() => [
    {
      key: nextSurgeonKey(),
      name: 'general',
      brief:
        'Handle one item: do the read-only homework, then hand back a finding or a draft with exact identifiers.',
      readOnly: true,
    },
  ]);
  const [lenses, setLenses] = useState<string[]>([...LENSES]);
  const [postCommand, setPostCommand] = useState('');
  const [approval, setApproval] = useState<'human' | 'none'>('human');
  const [commit, setCommit] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const pick = async () => {
    const d = await window.electron.agentWorkPickDir();
    if (d) {
      setDir(d);
      if (!name)
        setName(
          d
            .split('/')
            .filter(Boolean)
            .pop()
            ?.toLowerCase()
            .replace(/[^a-z0-9_-]/g, '-') ?? ''
        );
    }
  };
  const yaml = manifestYaml({
    name: name.trim() || 'desk',
    title,
    timezone,
    from,
    to,
    days,
    always,
    cadence,
    envFile,
    poll: poll
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean),
    guard: guard
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean),
    surgeons: surgeons
      .filter((s) => s.name.trim())
      .map(({ name, brief, readOnly }) => ({ name, brief, readOnly })),
    lenses,
    postCommand,
    approval,
    commit,
    extensions,
  });
  const create = async () => {
    setError(null);
    if (!dir.trim()) return setError('Pick a directory for the agent.');
    if (!/^[A-Za-z0-9_-]+$/.test(name.trim()))
      return setError('The name must be letters, digits, - or _.');
    if (!/^[1-9]\d*[smh]$/.test(cadence.trim())) return setError('Cadence is <n>s, <n>m or <n>h.');
    if (!charter.trim())
      return setError('Describe the agent’s purpose and boundaries in its charter.');
    try {
      new Intl.DateTimeFormat('en', { timeZone: timezone });
    } catch {
      return setError('Enter a valid timezone, such as Europe/London.');
    }
    if (
      !always &&
      (!days.length ||
        !/^([01]\d|2[0-3]):[0-5]\d$/.test(from) ||
        !/^([01]\d|2[0-3]):[0-5]\d$/.test(to))
    )
      return setError('Choose at least one day and valid 24-hour start and end times.');
    if (
      !surgeons.length ||
      surgeons.some((s) => !/^[A-Za-z0-9_-]+$/.test(s.name) || !s.brief.trim())
    )
      return setError('Every worker needs a valid name and a clear assignment.');
    if (new Set(surgeons.map((s) => s.name)).size !== surgeons.length)
      return setError('Worker names must be unique.');
    setBusy(true);
    try {
      const r = await window.electron.agentWorkInit(
        dir.trim(),
        yaml,
        charter.trim() ? `# Charter\n\n${charter.trim()}\n` : ''
      );
      if (!r.ok) return setError(r.error ?? 'could not create the agent');
      onCreated(dir.trim());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const approvalOptions: SegmentedOption<'human' | 'none'>[] = [
    { value: 'human', label: 'I approve every draft' },
    { value: 'none', label: 'post reviewed drafts next tick' },
  ];

  const cadencePick: CadencePick =
    CADENCE_PRESETS.find((p) => p.value === cadence.trim())?.value ?? 'custom';
  const cadenceOptions: SegmentedOption<CadencePick>[] = [
    ...CADENCE_PRESETS.map((p) => ({ value: p.value, label: p.label })),
    { value: 'custom', label: 'custom' },
  ];
  const cadenceInputRef = useRef<HTMLInputElement>(null);
  const zones = useMemo(() => timezoneOptions(), []);
  const tools = extensionsList.filter((extension) => extension.type !== 'platform');

  const toggleDay = (d: (typeof DAYS)[number]) => {
    const on = days.includes(d);
    setDays(
      on
        ? days.filter((x) => x !== d)
        : [...days, d].sort(
            (a, b) =>
              DAYS.indexOf(a as (typeof DAYS)[number]) - DAYS.indexOf(b as (typeof DAYS)[number])
          )
    );
  };

  const pill = (on: boolean, extra?: string) =>
    cx(
      'h-7 rounded-lz-pill px-2.5 text-[12px] disabled:pointer-events-none disabled:bg-lz-surface-2 disabled:text-lz-ink-3',
      WEIGHT.medium,
      on ? 'bg-lz-accent text-lz-accent-ink' : 'bg-lz-surface-2 text-lz-ink-2 hover:text-lz-ink',
      extra
    );

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Content
          aria-describedby={`${id}-subtitle`}
          aria-labelledby={`${id}-title`}
          className="fixed inset-0 z-50 flex flex-col bg-lz-surface pt-8 text-lz-ink"
          onEscapeKeyDown={(event) => {
            if (busy) event.preventDefault();
          }}
        >
          <header
            className={cx(
              'flex items-start justify-between gap-4 border-b px-6 py-4',
              SURFACE.hairline
            )}
          >
            <div className="flex min-w-0 flex-col gap-1">
              <Dialog.Title id={`${id}-title`} className={TYPE.h1}>
                Create an agent
              </Dialog.Title>
              <p id={`${id}-subtitle`} className={TYPE.bodyMuted}>
                A desk that wakes on a schedule, works its inbox with the tools you give it, and
                keeps everything in its own folder.
              </p>
            </div>
            <Button
              variant="ghost"
              iconOnly
              icon={<X />}
              aria-label="Close"
              onClick={onClose}
              disabled={busy}
            />
          </header>
          <div
            className={`grid min-h-0 flex-1 gap-0 overflow-auto ${preview ? 'lg:grid-cols-[minmax(0,1fr)_minmax(300px,35%)]' : 'grid-cols-1'}`}
          >
            <div className="mx-auto flex w-full max-w-4xl flex-col gap-5 p-6 lg:p-8">
              <div>
                <h2 className={cx(sectionTitle, 'mb-1')}>Purpose & workspace</h2>
                <p className={cx('mb-4', TYPE.bodyMuted)}>
                  Give your agent a clear assignment. Its research, drafts and history stay in its
                  own folder.
                </p>
                <span className={label}>Directory</span>
                <div className="flex gap-2">
                  <input
                    ref={firstRef}
                    className={field}
                    value={dir}
                    onChange={(e) => setDir(e.target.value)}
                    placeholder="~/goose-agents/web-research"
                    aria-label="Agent directory"
                  />
                  <Button variant="secondary" icon={<FolderOpen />} onClick={pick}>
                    Pick
                  </Button>
                </div>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <span className={label}>Name</span>
                  <input
                    className={field}
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    placeholder="web-research"
                    aria-label="Agent name"
                  />
                </div>
                <div>
                  <span className={label}>Title</span>
                  <input
                    className={field}
                    value={title}
                    onChange={(e) => setTitle(e.target.value)}
                    placeholder="Web research"
                    aria-label="Agent title"
                  />
                </div>
              </div>
              <div>
                <span className={label}>Charter — the desk's rules (CHARTER.md)</span>
                <textarea
                  className={cx(field, 'h-24 py-1')}
                  value={charter}
                  onChange={(e) => setCharter(e.target.value)}
                  placeholder="Who it speaks as, what it may touch, what it must ask about first."
                  aria-label="Charter"
                />
              </div>

              <h2 className={cx(sectionTitle, 'mt-4')}>Schedule</h2>
              <div>
                <span className={label}>How often it wakes</span>
                <div className="flex flex-wrap items-center gap-2">
                  <Segmented<CadencePick>
                    aria-label="Cadence presets"
                    options={cadenceOptions}
                    value={cadencePick}
                    onChange={(v) => {
                      if (v === 'custom') cadenceInputRef.current?.focus();
                      else setCadence(v);
                    }}
                  />
                  <input
                    ref={cadenceInputRef}
                    className={cx(field, 'w-24 font-mono')}
                    value={cadence}
                    onChange={(e) => setCadence(e.target.value)}
                    aria-label="Cadence"
                    aria-describedby={`${id}-cadence-hint`}
                  />
                  <span id={`${id}-cadence-hint`} className={TYPE.meta}>
                    custom: a number and s, m or h (90m, 2h)
                  </span>
                </div>
              </div>
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_minmax(0,1fr)]">
                <div>
                  <span className={label}>Timezone</span>
                  <Combobox
                    aria-label="Timezone"
                    options={zones}
                    value={timezone}
                    onChange={setTimezone}
                    allowFreeText
                    placeholder="Europe/London"
                    emptyText="No zone matches — the name is checked when you create the agent."
                  />
                </div>
                <div>
                  <span className={label}>From</span>
                  <input
                    className={cx(field, 'disabled:bg-lz-surface-2 disabled:text-lz-ink-3')}
                    value={from}
                    onChange={(e) => setFrom(e.target.value)}
                    aria-label="Window from"
                    disabled={always}
                  />
                </div>
                <div>
                  <span className={label}>To</span>
                  <input
                    className={cx(field, 'disabled:bg-lz-surface-2 disabled:text-lz-ink-3')}
                    value={to}
                    onChange={(e) => setTo(e.target.value)}
                    aria-label="Window to"
                    disabled={always}
                  />
                </div>
              </div>
              <div className="flex flex-wrap items-center gap-1.5">
                {DAYS.map((d) => {
                  const on = days.includes(d);
                  return (
                    <button
                      key={d}
                      type="button"
                      aria-pressed={on}
                      disabled={always}
                      onClick={() => toggleDay(d)}
                      className={pill(on)}
                    >
                      {d}
                    </button>
                  );
                })}
                <button
                  type="button"
                  aria-pressed={always}
                  onClick={() => setAlways(!always)}
                  className={pill(always, 'ml-2')}
                >
                  always open
                </button>
              </div>

              <h2 className={cx(sectionTitle, 'mt-4')}>Tools & access</h2>
              <p className={TYPE.bodyMuted}>
                Choose saved MCP configurations. Set up credentials and verify connections in the
                MCPs menu first. Only select tools this agent should use.
              </p>
              <div className="grid gap-2 sm:grid-cols-2">
                {tools.map((extension) => (
                  <Checkbox
                    key={extension.name}
                    variant="card"
                    label={getFriendlyTitle(extension)}
                    description={getSubtitle(extension).description ?? undefined}
                    checked={extensions.includes(extension.name)}
                    onChange={(checked) =>
                      setExtensions(
                        checked
                          ? [...extensions, extension.name]
                          : extensions.filter((name) => name !== extension.name)
                      )
                    }
                  />
                ))}
              </div>
              <Disclosure title="Advanced workflow: scripts, workers and approval">
                <div className="flex flex-col gap-5">
                  <div>
                    <span className={label}>Script environment file (KEY=VALUE)</span>
                    <input
                      className={field}
                      value={envFile}
                      onChange={(e) => setEnvFile(e.target.value)}
                      placeholder="references/credentials.env"
                      aria-label="Env file"
                    />
                  </div>
                  <div>
                    <span className={label}>
                      Poll — read-only commands, one per line; their stdout is the tick's inbox
                    </span>
                    <textarea
                      className={cx(field, 'h-20 py-1 font-mono')}
                      value={poll}
                      onChange={(e) => setPoll(e.target.value)}
                      placeholder={'python3 scripts/inbox.py'}
                      aria-label="Poll commands"
                    />
                  </div>
                  <div>
                    <span className={label}>Guard — commands run first; exit 3 holds the tick</span>
                    <textarea
                      className={cx(field, 'h-14 py-1 font-mono')}
                      value={guard}
                      onChange={(e) => setGuard(e.target.value)}
                      placeholder={'bash scripts/status.sh'}
                      aria-label="Guard commands"
                    />
                  </div>
                  <div>
                    <div className="flex items-center justify-between">
                      <span className={label}>Workers — specialist assignments</span>
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={<Plus />}
                        onClick={() =>
                          setSurgeons([
                            ...surgeons,
                            { key: nextSurgeonKey(), name: '', brief: '', readOnly: true },
                          ])
                        }
                      >
                        add
                      </Button>
                    </div>
                    <div className="flex flex-col gap-2">
                      {surgeons.map((s, i) => (
                        <div
                          key={s.key}
                          className="grid grid-cols-[140px_1fr_auto_auto] items-start gap-2"
                        >
                          <input
                            className={field}
                            value={s.name}
                            onChange={(e) =>
                              setSurgeons(
                                surgeons.map((x, j) =>
                                  j === i ? { ...x, name: e.target.value } : x
                                )
                              )
                            }
                            placeholder="access"
                            aria-label={`Surgeon ${i + 1} name`}
                          />
                          <input
                            className={field}
                            value={s.brief}
                            onChange={(e) =>
                              setSurgeons(
                                surgeons.map((x, j) =>
                                  j === i ? { ...x, brief: e.target.value } : x
                                )
                              )
                            }
                            placeholder="what this surgeon handles and returns"
                            aria-label={`Surgeon ${i + 1} brief`}
                          />
                          <button
                            type="button"
                            aria-pressed={s.readOnly}
                            onClick={() =>
                              setSurgeons(
                                surgeons.map((x, j) =>
                                  j === i ? { ...x, readOnly: !x.readOnly } : x
                                )
                              )
                            }
                            className={cx(
                              'h-8 rounded-lz-control px-2 text-[12px]',
                              WEIGHT.medium,
                              s.readOnly
                                ? 'bg-lz-ok-solid text-white'
                                : 'bg-lz-warn-solid text-white'
                            )}
                          >
                            {s.readOnly ? 'restricted tools' : 'may write'}
                          </button>
                          <Button
                            variant="ghost"
                            size="sm"
                            iconOnly
                            icon={<X />}
                            aria-label={`Remove surgeon ${i + 1}`}
                            onClick={() => setSurgeons(surgeons.filter((_, j) => j !== i))}
                          />
                        </div>
                      ))}
                    </div>
                  </div>
                  <div>
                    <span className={label}>Review lenses — each one attacks every draft</span>
                    <div className="flex flex-wrap gap-1.5">
                      {LENSES.map((l) => {
                        const on = lenses.includes(l);
                        return (
                          <button
                            key={l}
                            type="button"
                            aria-pressed={on}
                            onClick={() =>
                              setLenses(on ? lenses.filter((x) => x !== l) : [...lenses, l])
                            }
                            className={pill(on)}
                          >
                            {l}
                          </button>
                        );
                      })}
                    </div>
                  </div>
                  <div>
                    <span className={label}>
                      Posting adapter command ({'{id}'} = draft id, {'{file}'} = its body)
                    </span>
                    <input
                      className={cx(field, 'font-mono')}
                      value={postCommand}
                      onChange={(e) => setPostCommand(e.target.value)}
                      placeholder="python3 scripts/post_comment.py --prepared {id}"
                      aria-label="Post command"
                    />
                    <div className="mt-2">
                      <Segmented
                        size="sm"
                        options={approvalOptions}
                        value={approval}
                        onChange={setApproval}
                        aria-label="Approval"
                      />
                    </div>
                  </div>
                  <Checkbox
                    checked={commit}
                    onChange={setCommit}
                    label="Commit the agent directory after every tick"
                  />
                  <p className={TYPE.meta}>
                    Restricted tools are not a sandbox. Shell commands and selected MCPs retain
                    their own permissions. Posting adapters must enforce the destination’s approval
                    rules.
                  </p>
                </div>
              </Disclosure>
              {error && (
                <div
                  className={cx(
                    'rounded-lz-control bg-lz-err-solid px-3 py-2 text-lz-body text-white',
                    WEIGHT.medium
                  )}
                  role="alert"
                >
                  {error}
                </div>
              )}
            </div>
            {preview && (
              <div className="flex min-h-0 flex-col bg-lz-surface-2 p-6">
                <div className="mb-2 flex items-center justify-between">
                  <span className={TYPE.zone}>agent.yaml</span>
                  <Chip>{yaml.split('\n').length} lines</Chip>
                </div>
                <pre className={cx(TYPE.mono, 'min-h-0 flex-1 overflow-auto whitespace-pre-wrap')}>
                  {yaml}
                </pre>
              </div>
            )}
          </div>
          <div className="flex items-center justify-end gap-2 border-t border-lz-border px-4 py-3">
            <Button
              className="mr-auto"
              variant="ghost"
              onClick={() => setPreview(!preview)}
              aria-pressed={preview}
            >
              {preview ? 'Hide configuration' : 'Preview configuration'}
            </Button>
            <Button variant="secondary" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button variant="primary" onClick={create} disabled={busy}>
              Create agent
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
