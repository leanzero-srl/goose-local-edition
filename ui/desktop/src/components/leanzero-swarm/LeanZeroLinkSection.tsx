import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowLeft,
  Check,
  ChevronDown,
  Laptop,
  Link2,
  Loader2,
  LogOut,
  Mail,
  Play,
  RefreshCw,
  Users,
} from 'lucide-react';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { defineMessages, useIntl } from '../../i18n';
import {
  Button,
  Chip,
  DataTable,
  EmptyState,
  KeyValue,
  Panel,
  StatusDot,
  DISABLED,
  FOCUS,
  MOTION,
  RADIUS,
  ROW,
  SURFACE,
  TNUM,
  TONE_FILL,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type DataTableColumn,
  type Tone,
} from '../lz';
import { FIELD_LABEL, INPUT, TEXTAREA, ToneBanner, nodeHue } from './studio';
import {
  leanzeroLinkConnect,
  leanzeroLinkHealth,
  leanzeroLinkLogout,
  leanzeroLinkNodes,
  leanzeroLinkRemoteExecute,
  leanzeroLinkRequestCode,
  leanzeroLinkStatus,
  leanzeroLinkVerify,
  linkBannerText,
  linkErrorText,
  type AuthState,
  type LinkHealth,
  type LinkState,
  type NodeState,
  type NodeStatus,
  type NodesResponse,
} from '../../acp/leanzero-link';

const i18n = defineMessages({
  tagline: {
    id: 'leanzeroLink.tagline',
    defaultMessage:
      'Sign in to link your devices into a private mesh — no password, just a code by email.',
  },
});

// ---------------------------------------------------------------------------
// Pure helpers.
// ---------------------------------------------------------------------------

const CODE_LENGTH = 6;

/** Consecutive failed status() polls (≈3s each) before the connected view flags staleness. */
const STALE_POLL_THRESHOLD = 3;

function emailOf(auth: AuthState): string {
  return 'email' in auth ? auth.email : '';
}

/** `mihai@wolfaenpak.com` → `m****@wolfaenpak.com`. Never invents; empty stays empty. */
function maskEmail(email: string): string {
  const at = email.indexOf('@');
  if (at <= 0) return email;
  const local = email.slice(0, at);
  const domain = email.slice(at);
  const head = local.slice(0, 1);
  return `${head}${'*'.repeat(Math.max(1, local.length - 1))}${domain}`;
}

function formatCountdown(totalSeconds: number): string {
  const s = Math.max(0, totalSeconds);
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return `${m}:${rem.toString().padStart(2, '0')}`;
}

/** Honest relative age of a snake_case `updated_at` / `lastSeen` ISO timestamp. */
function formatLastSeen(iso: string | undefined, now: number): string {
  if (!iso) return 'unknown';
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  const secs = Math.max(0, Math.round((now - t) / 1000));
  if (secs < 5) return 'just now';
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

interface StatusVisual {
  tone: Extract<Tone, 'ok' | 'warn' | 'stopped'>;
  label: string;
  sessionId?: string;
}

function nodeStatusVisual(status: NodeStatus): StatusVisual {
  switch (status.type) {
    case 'Idle':
      return { tone: 'ok', label: 'idle' };
    case 'Busy':
      return { tone: 'warn', label: 'busy', sessionId: status.session_id };
    case 'Offline':
    default:
      return { tone: 'stopped', label: 'offline' };
  }
}

// ---------------------------------------------------------------------------
// Shared chrome (LeanZero Studio: Panels, status triad, one accent, custom controls).
// Declared at module scope — never inside a render body.
// ---------------------------------------------------------------------------

function StatusChip({ status }: { status: NodeStatus }) {
  const v = nodeStatusVisual(status);
  return (
    <Chip tone={v.tone} title={v.sessionId ? `busy on session ${v.sessionId}` : v.label}>
      {v.label}
      {v.sessionId ? ` · ${v.sessionId.slice(0, 8)}` : ''}
    </Chip>
  );
}

function MeshIp({ ip }: { ip: string | undefined }) {
  return ip ? (
    <span className="font-mono text-lz-mono text-lz-ink">{ip}</span>
  ) : (
    <span className="text-lz-ink-4">no mesh IP</span>
  );
}

// Custom checkbox (no native input, per the design bans). Solid accent fill when checked.
function WipeCheckbox({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={cx('flex items-center gap-2 text-left', TYPE.body, FOCUS)}
      data-testid="link-wipe-checkbox"
    >
      <span
        className={cx(
          'flex size-4 shrink-0 items-center justify-center rounded-[4px] border [&_svg]:size-3',
          checked ? 'border-lz-accent bg-lz-accent text-lz-accent-ink' : 'border-lz-border-strong bg-lz-surface',
          MOTION
        )}
      >
        {checked && <Check />}
      </span>
      <span>{label}</span>
    </button>
  );
}

// ---------------------------------------------------------------------------
// Login card (spans loggedOut → codeSent). Owns its email/code input + countdown.
// ---------------------------------------------------------------------------

interface LoginCardProps {
  auth: Extract<AuthState, { state: 'loggedOut' } | { state: 'codeSent' }>;
  submitting: boolean;
  error: string | null;
  onRequestCode: (email: string) => Promise<boolean>;
  onVerify: (email: string, code: string) => Promise<void>;
}

function LoginCard({ auth, submitting, error, onRequestCode, onVerify }: LoginCardProps) {
  const intl = useIntl();
  const [email, setEmail] = useState(emailOf(auth));
  const [code, setCode] = useState('');
  const [backToEmail, setBackToEmail] = useState(false);
  const [now, setNow] = useState(() => Date.now());

  const codeStage = auth.state === 'codeSent' && !backToEmail;
  const codeEmail = auth.state === 'codeSent' ? auth.email : email;

  useEffect(() => {
    if (!codeStage) return undefined;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [codeStage]);

  const remainingSecs =
    auth.state === 'codeSent'
      ? Math.max(0, Math.round((Date.parse(auth.expiresAt) - now) / 1000))
      : 0;
  const expired = codeStage && remainingSecs <= 0;

  const submitEmail = async () => {
    const trimmed = email.trim();
    if (!trimmed || submitting) return;
    const ok = await onRequestCode(trimmed);
    if (ok) {
      setBackToEmail(false);
      setCode('');
    }
  };

  const digits = code.replace(/\D/g, '').slice(0, CODE_LENGTH);
  const canVerify = digits.length === CODE_LENGTH && !submitting;

  return (
    <div className="mx-auto w-full max-w-md" data-testid="link-login-card">
      <Panel padded={false}>
        <div
          className={cx(
            'flex flex-col items-center gap-2 border-b px-6 py-6 text-center',
            SURFACE.hairline
          )}
        >
          <span
            aria-hidden
            className={cx(
              'mb-1 flex size-12 items-center justify-center [&_svg]:size-6',
              RADIUS.card,
              TONE_FILL.accent
            )}
          >
            <Link2 />
          </span>
          <h2 className={TYPE.h1}>LeanZero Link</h2>
          <p className={cx(TYPE.bodyMuted, 'max-w-[42ch]')}>{intl.formatMessage(i18n.tagline)}</p>
        </div>

        <div className="flex flex-col gap-4 px-6 py-6">
          {error && <ToneBanner tone="err" label="Sign-in" text={error} />}

          {!codeStage ? (
            <form
              className="flex flex-col gap-3"
              onSubmit={(e) => {
                e.preventDefault();
                void submitEmail();
              }}
            >
              <label htmlFor="link-email-input" className={FIELD_LABEL}>
                Email
              </label>
              <input
                id="link-email-input"
                data-testid="link-email-input"
                type="email"
                autoComplete="email"
                inputMode="email"
                placeholder="you@example.com"
                value={email}
                disabled={submitting}
                onChange={(e) => setEmail(e.target.value)}
                className={cx(INPUT, 'w-full')}
              />
              <Button
                variant="primary"
                type="submit"
                disabled={submitting || email.trim() === ''}
                data-testid="link-send-code"
                className="w-full"
                icon={submitting ? <Loader2 className="animate-spin" /> : <Mail />}
              >
                Send code
              </Button>
            </form>
          ) : (
            <div className="flex flex-col gap-3">
              <div className="flex items-center justify-between gap-2">
                <span className={TYPE.bodyMuted}>
                  Code sent to{' '}
                  <span
                    className={cx('text-lz-ink', WEIGHT.semibold)}
                    data-testid="link-masked-email"
                  >
                    {maskEmail(codeEmail)}
                  </span>
                </span>
                <span
                  data-testid="link-countdown"
                  className={cx(
                    'inline-flex h-6 items-center px-2 text-lz-body',
                    WEIGHT.semibold,
                    TNUM,
                    RADIUS.control,
                    expired ? TONE_FILL.err : TONE_FILL.accent
                  )}
                  title={expired ? 'the code has expired' : 'time until the code expires'}
                >
                  {formatCountdown(remainingSecs)}
                </span>
              </div>
              <label htmlFor="link-code-input" className={FIELD_LABEL}>
                6-digit code
              </label>
              <input
                id="link-code-input"
                data-testid="link-code-input"
                inputMode="numeric"
                autoComplete="one-time-code"
                placeholder="000000"
                maxLength={CODE_LENGTH}
                value={digits}
                disabled={submitting}
                onChange={(e) => setCode(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && canVerify) void onVerify(codeEmail, digits);
                }}
                className={cx(
                  'h-11 w-full bg-lz-surface px-3 text-center font-mono text-[22px] tracking-[0.5em] text-lz-ink placeholder:text-lz-ink-4',
                  SURFACE.outline,
                  RADIUS.control,
                  FOCUS,
                  MOTION,
                  DISABLED
                )}
              />
              {expired && (
                <span className={cx('text-lz-meta', WEIGHT.medium, TONE_TEXT.err)}>
                  This code has expired — send a new one.
                </span>
              )}
              <Button
                variant="primary"
                type="button"
                disabled={!canVerify}
                data-testid="link-verify"
                onClick={() => void onVerify(codeEmail, digits)}
                className="w-full"
                icon={submitting ? <Loader2 className="animate-spin" /> : <Check />}
              >
                Verify
              </Button>
              <div className="flex items-center justify-between gap-2">
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  data-testid="link-resend"
                  disabled={submitting}
                  onClick={() => void onRequestCode(codeEmail)}
                  icon={<RefreshCw />}
                >
                  Send a new code
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  data-testid="link-different-email"
                  disabled={submitting}
                  onClick={() => {
                    setBackToEmail(true);
                    setCode('');
                  }}
                  icon={<ArrowLeft />}
                >
                  Use a different email
                </Button>
              </div>
            </div>
          )}
        </div>
      </Panel>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Connect card (loggedIn) and connecting state.
// ---------------------------------------------------------------------------

function ConnectCard({
  email,
  connecting,
  error,
  audienceSyncFailed,
  onConnect,
}: {
  email: string;
  connecting: boolean;
  error: string | null;
  audienceSyncFailed: boolean;
  onConnect: () => void;
}) {
  return (
    <div className="mx-auto w-full max-w-md" data-testid="link-connect-card">
      <Panel title="Signed in">
        <div className="flex flex-col gap-4">
          <KeyValue
            aria-label="Account"
            items={[
              { key: 'account', label: 'Account', value: email },
              {
                key: 'mesh',
                label: 'Mesh',
                value: (
                  <span className="inline-flex items-center gap-1.5">
                    <StatusDot tone="stopped" label="not connected" />
                    not connected
                  </span>
                ),
              },
            ]}
          />

          {audienceSyncFailed && (
            <ToneBanner
              tone="warn"
              label="Audience"
              text="Signed in, but syncing your contact info to LeanZero didn't go through. Your account works — this is just the mailing audience."
              testId="link-audience-note"
            />
          )}

          {error && <ToneBanner tone="err" label="Connect failed" text={error} />}

          <p className={TYPE.bodyMuted}>
            Bring this Mac onto your private mesh so your linked devices can see each other.
          </p>

          <Button
            variant="primary"
            type="button"
            disabled={connecting}
            data-testid="link-connect"
            onClick={onConnect}
            className="w-full"
            icon={connecting ? <Loader2 className="animate-spin" /> : <Link2 />}
          >
            {error ? 'Retry connect' : 'Connect to mesh'}
          </Button>
        </div>
      </Panel>
    </div>
  );
}

function ConnectingCard({ email }: { email: string }) {
  return (
    <div className="mx-auto w-full max-w-md" data-testid="link-connecting">
      <Panel title="Connecting">
        <div className="flex flex-col items-center gap-3 py-6 text-center">
          <Loader2 className="size-8 animate-spin text-lz-accent" />
          <span className={cx(TYPE.body, WEIGHT.semibold)}>Joining your private mesh</span>
          <span className={TYPE.meta}>{email}</span>
        </div>
      </Panel>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Run-a-prompt-on-a-device card (P5 made tangible) + its custom node dropdown.
// ---------------------------------------------------------------------------

/** One entry in the target dropdown — self (always runnable) or a peer (only when Idle). */
interface RunTargetOption {
  nodeId: string;
  hostname: string;
  isSelf: boolean;
  status: NodeStatus;
  /** Self and Idle peers are selectable; Busy/Offline peers show DISABLED with a reason. */
  selectable: boolean;
  /** Why a non-selectable peer can't be targeted right now ("busy" / "offline"). */
  reason?: string;
}

function targetLabel(opt: RunTargetOption): string {
  return opt.isSelf ? `This device · ${opt.hostname}` : opt.hostname;
}

/**
 * Custom device dropdown — never a native <select>. Busy/Offline peers are rendered
 * DISABLED (honest: shown, not hidden) with their live state as the reason; self and Idle
 * peers are pickable. Studio chrome: the outline control, status chips, the overlay surface.
 */
function NodeSelect({
  options,
  value,
  onChange,
  disabled,
}: {
  options: RunTargetOption[];
  value: string | null;
  onChange: (nodeId: string) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return undefined;
    const onDocMouseDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDocMouseDown);
    return () => document.removeEventListener('mousedown', onDocMouseDown);
  }, [open]);

  const selected = options.find((o) => o.nodeId === value) ?? null;

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        data-testid="link-run-target"
        disabled={disabled || options.length === 0}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={cx(
          'flex h-9 w-full items-center gap-2 bg-lz-surface px-3 text-left text-lz-body text-lz-ink [&>svg]:size-4 [&>svg]:shrink-0 [&>svg]:text-lz-ink-3',
          SURFACE.outline,
          RADIUS.control,
          DISABLED,
          FOCUS,
          MOTION
        )}
      >
        <Laptop />
        <span className={cx('min-w-0 truncate', WEIGHT.medium)}>
          {selected ? targetLabel(selected) : 'No runnable device'}
        </span>
        {selected && <StatusChip status={selected.status} />}
        <ChevronDown className="ml-auto" />
      </button>
      {open && (
        <div
          role="listbox"
          aria-label="Target device"
          className={cx(
            'absolute left-0 top-full z-[60] mt-1 w-full overflow-hidden p-1',
            SURFACE.overlay
          )}
        >
          {options.map((opt) => (
            <button
              key={opt.nodeId}
              type="button"
              role="option"
              aria-selected={opt.nodeId === value}
              aria-disabled={!opt.selectable}
              disabled={!opt.selectable}
              data-testid={`link-run-target-option-${opt.nodeId}`}
              title={opt.selectable ? targetLabel(opt) : `${targetLabel(opt)} — ${opt.reason}`}
              onClick={() => {
                if (!opt.selectable) return;
                onChange(opt.nodeId);
                setOpen(false);
              }}
              className={cx(
                'flex w-full items-center gap-2 px-2.5 text-left text-lz-body text-lz-ink disabled:cursor-not-allowed disabled:text-lz-ink-3 [&>svg]:size-4 [&>svg]:shrink-0 [&>svg]:text-lz-ink-3',
                ROW.dense,
                RADIUS.control,
                opt.nodeId === value ? SURFACE.inset : SURFACE.hover,
                MOTION
              )}
            >
              <Laptop />
              <span className={cx('min-w-0 truncate', WEIGHT.medium)}>{targetLabel(opt)}</span>
              <StatusChip status={opt.status} />
              {!opt.selectable && opt.reason && (
                <span className={cx('ml-auto shrink-0', TYPE.meta)}>can&apos;t run — {opt.reason}</span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function buildRunTargets(nodes: NodesResponse | null): RunTargetOption[] {
  if (!nodes?.self) return [];
  const self: RunTargetOption = {
    nodeId: nodes.self.node_id,
    hostname: nodes.self.hostname,
    isSelf: true,
    status: nodes.self.status,
    selectable: true,
  };
  const peers: RunTargetOption[] = (nodes.peers ?? []).map((p) => {
    const idle = p.status.type === 'Idle';
    return {
      nodeId: p.node_id,
      hostname: p.hostname,
      isSelf: false,
      status: p.status,
      selectable: idle,
      reason: idle ? undefined : p.status.type === 'Busy' ? 'busy' : 'offline',
    };
  });
  return [self, ...peers];
}

/**
 * Below the device list: pick an idle device, type a prompt, Run. Fires
 * `leanzeroLinkRemoteExecute` and confirms with the started session id (the deltas already
 * mirror here via P4 — no stream viewer is built). Errors ("node is busy", "remote
 * execution disabled on this node", …) ride through VERBATIM via `linkBannerText`.
 */
function RunPromptCard({ nodes }: { nodes: NodesResponse | null }) {
  const options = useMemo(() => buildRunTargets(nodes), [nodes]);
  const [targetNodeId, setTargetNodeId] = useState<string | null>(null);
  const [prompt, setPrompt] = useState('');
  const [workingDir, setWorkingDir] = useState('');
  const [inFlight, setInFlight] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{ sessionId: string; hostname: string } | null>(null);

  // Keep the selection valid: preserve it while still selectable, else default to self
  // (always runnable), else the first selectable, else nothing (dropdown/Run disabled).
  useEffect(() => {
    const selectable = options.filter((o) => o.selectable);
    if (selectable.length === 0) {
      setTargetNodeId(null);
      return;
    }
    setTargetNodeId((prev) => {
      if (prev && selectable.some((o) => o.nodeId === prev)) return prev;
      return (selectable.find((o) => o.isSelf) ?? selectable[0]).nodeId;
    });
  }, [options]);

  const trimmedPrompt = prompt.trim();
  const selectedSelectable =
    targetNodeId != null && options.some((o) => o.nodeId === targetNodeId && o.selectable);
  const canRun = !inFlight && trimmedPrompt !== '' && selectedSelectable;

  const run = useCallback(async () => {
    if (inFlight || targetNodeId == null || trimmedPrompt === '' || !selectedSelectable) return;
    const target = options.find((o) => o.nodeId === targetNodeId);
    setError(null);
    setResult(null);
    setInFlight(true);
    try {
      const res = await leanzeroLinkRemoteExecute({
        targetNodeId,
        prompt: trimmedPrompt,
        workingDir,
      });
      setResult({ sessionId: res.sessionId, hostname: target?.hostname ?? targetNodeId });
      setPrompt('');
    } catch (e) {
      setError(linkBannerText(e));
    } finally {
      setInFlight(false);
    }
  }, [inFlight, targetNodeId, trimmedPrompt, selectedSelectable, options, workingDir]);

  return (
    <Panel title="Run a prompt on a linked device">
      <div className="flex flex-col gap-3" data-testid="link-run-card">
        <span className={FIELD_LABEL}>Device</span>
        <NodeSelect
          options={options}
          value={targetNodeId}
          onChange={setTargetNodeId}
          disabled={inFlight}
        />

        <label htmlFor="link-run-prompt" className={FIELD_LABEL}>
          Prompt
        </label>
        <textarea
          id="link-run-prompt"
          data-testid="link-run-prompt"
          rows={3}
          value={prompt}
          disabled={inFlight}
          placeholder="Describe what this device should do…"
          onChange={(e) => setPrompt(e.target.value)}
          className={TEXTAREA}
        />

        <label htmlFor="link-run-workdir" className={FIELD_LABEL}>
          Working directory (optional)
        </label>
        <input
          id="link-run-workdir"
          data-testid="link-run-workdir"
          type="text"
          value={workingDir}
          disabled={inFlight}
          placeholder="defaults to the device's home"
          onChange={(e) => setWorkingDir(e.target.value)}
          className={cx(INPUT, 'w-full font-mono')}
        />

        {error && (
          <div data-testid="link-run-error">
            <ToneBanner tone="err" label="Run" text={error} />
          </div>
        )}

        {result && (
          <ToneBanner
            tone="ok"
            label="Started"
            text={`Started session ${result.sessionId} on ${result.hostname} — its activity will mirror here.`}
            testId="link-run-success"
          />
        )}

        <Button
          variant="primary"
          type="button"
          disabled={!canRun}
          data-testid="link-run-submit"
          onClick={() => void run()}
          className="w-full"
          icon={inFlight ? <Loader2 className="animate-spin" /> : <Play />}
        >
          Run
        </Button>
      </div>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Connected dashboard.
// ---------------------------------------------------------------------------

/** A peer row in the Linked devices table, with its identity hue by list position. */
interface PeerRow {
  node: NodeState;
  hue: ReturnType<typeof nodeHue>;
}

const PEER_COLUMNS = (now: number): DataTableColumn<PeerRow>[] => [
  {
    key: 'device',
    header: 'Device',
    cell: ({ node, hue }) => (
      <span className="flex items-center gap-2">
        <StatusDot node={hue} label={`node ${node.hostname}`} />
        <span className={cx('truncate', WEIGHT.semibold)}>{node.hostname}</span>
      </span>
    ),
  },
  { key: 'status', header: 'Status', cell: ({ node }) => <StatusChip status={node.status} /> },
  { key: 'ip', header: 'Mesh IP', cell: ({ node }) => <MeshIp ip={node.mesh_ip} /> },
  {
    key: 'sessions',
    header: 'Sessions',
    numeric: true,
    cell: ({ node }) => node.sessions_active,
  },
  {
    key: 'seen',
    header: 'Last seen',
    align: 'right',
    cell: ({ node }) => (
      <span className={cx(TYPE.meta, TNUM)}>{formatLastSeen(node.updated_at, now)}</span>
    ),
  },
];

function ConnectedView({
  email,
  linkState,
  nodes,
  now,
  stale,
  onLogout,
}: {
  email: string;
  linkState: LinkState;
  nodes: NodesResponse | null;
  now: number;
  stale: boolean;
  onLogout: () => void;
}) {
  const mesh = linkState.mesh;
  const peers = nodes?.peers ?? [];
  const peerRows: PeerRow[] = peers.map((node, i) => ({ node, hue: nodeHue(i + 1) }));
  const columns = useMemo(() => PEER_COLUMNS(now), [now]);
  const self = nodes?.self;

  return (
    <div className="flex flex-col gap-4 pb-8" data-testid="link-connected">
      {stale && (
        <ToneBanner
          tone="warn"
          label="Reconnecting"
          text="Reconnecting… (lost contact with the local node)"
          live
          testId="link-reconnecting"
        />
      )}
      <Panel
        title="Account"
        headerRight={
          <Button
            type="button"
            size="sm"
            variant="secondary"
            data-testid="link-logout"
            onClick={onLogout}
            icon={<LogOut />}
          >
            Log out / Switch account
          </Button>
        }
      >
        <KeyValue
          aria-label="Account"
          items={[
            { key: 'account', label: 'Account', value: email },
            {
              key: 'mesh',
              label: 'Mesh',
              value: (
                <span className="inline-flex items-center gap-1.5" data-testid="link-mesh-line">
                  <StatusDot
                    tone={mesh?.online ? 'ok' : 'stopped'}
                    label={mesh?.online ? 'mesh online' : 'mesh offline'}
                  />
                  mesh {mesh?.backendState ?? 'unknown'}
                  {mesh?.online ? ' · online' : ' · offline'}
                  {' · '}
                  {linkState.nodeCount} node{linkState.nodeCount === 1 ? '' : 's'}
                </span>
              ),
            },
          ]}
        />
      </Panel>

      {linkState.lastError && <ToneBanner tone="err" label="Mesh" text={linkState.lastError} />}

      <Panel title="This device">
        {self ? (
          <div data-testid="link-self">
            <KeyValue
              aria-label="This device"
              items={[
                {
                  key: 'device',
                  label: 'Device',
                  value: (
                    <span className="inline-flex items-center gap-2">
                      <StatusDot node={nodeHue(0)} label={`node ${self.hostname}`} />
                      {self.hostname}
                    </span>
                  ),
                },
                { key: 'state', label: 'State', value: <StatusChip status={self.status} /> },
                { key: 'ip', label: 'Mesh IP', value: <MeshIp ip={self.mesh_ip} /> },
                { key: 'sessions', label: 'Sessions active', value: self.sessions_active },
              ]}
            />
          </div>
        ) : (
          <div className={cx('flex items-center gap-2', TYPE.bodyMuted)}>
            <Loader2 className="size-4 animate-spin text-lz-accent" />
            Reading this device&apos;s state…
          </div>
        )}
      </Panel>

      <Panel title="Linked devices" count={peers.length} padded={false}>
        <div data-testid="link-peers">
          <DataTable
            aria-label="Linked devices"
            columns={columns}
            rows={peerRows}
            rowKey={(r) => r.node.node_id}
            empty={
              <div data-testid="link-peers-empty">
                <EmptyState
                  icon={<Users />}
                  title="No other devices linked yet"
                  body="Sign in on another Mac to see it here."
                />
              </div>
            }
          />
        </div>
      </Panel>

      <RunPromptCard nodes={nodes} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// The tab.
// ---------------------------------------------------------------------------

const LeanZeroLinkSection: React.FC = () => {
  const [linkState, setLinkState] = useState<LinkState | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [nodes, setNodes] = useState<NodesResponse | null>(null);
  const [health, setHealth] = useState<LinkHealth | null>(null);

  const [submitting, setSubmitting] = useState(false);
  const [authError, setAuthError] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [connectError, setConnectError] = useState<string | null>(null);
  const [audienceSyncFailed, setAudienceSyncFailed] = useState(false);

  const [logoutOpen, setLogoutOpen] = useState(false);
  const [wipe, setWipe] = useState(false);
  const [loggingOut, setLoggingOut] = useState(false);

  // A 1s clock for the connected view's relative last-seen labels.
  const [now, setNow] = useState(() => Date.now());

  // Staleness gate: count CONSECUTIVE failed status() polls. A single blip must not yank
  // the user out of the connected view, so we DEBOUNCE — only after N in a row do we
  // surface the "Reconnecting…" strip. A successful poll resets it. Auth transitions ride
  // the success path (setLinkState) and are never debounced; only the error case is.
  const [staleFailures, setStaleFailures] = useState(0);

  const disposedRef = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const next = await leanzeroLinkStatus();
      if (disposedRef.current) return;
      setLinkState(next);
      setStatusError(null);
      setStaleFailures(0);
      if (next.auth.state === 'connected') {
        try {
          const n = await leanzeroLinkNodes();
          if (!disposedRef.current) setNodes(n);
        } catch {
          // A failed nodes poll does not invalidate "connected" — keep the last roster.
        }
      } else if (!disposedRef.current) {
        setNodes(null);
      }
    } catch (e) {
      if (disposedRef.current) return;
      // Keep the last known state; surface the read failure as a truth line and count it
      // toward the staleness debounce (the connected view flips to "Reconnecting…" at N).
      setStatusError(linkErrorText(e));
      setStaleFailures((n) => n + 1);
    }
  }, []);

  // Poll status (+nodes when connected) every 3s while the tab is visible; stop when hidden.
  useEffect(() => {
    disposedRef.current = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    const start = () => {
      if (timer != null) return;
      void refresh();
      timer = setInterval(() => void refresh(), 3000);
    };
    const stop = () => {
      if (timer != null) {
        clearInterval(timer);
        timer = null;
      }
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') start();
      else stop();
    };
    onVisibility();
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      disposedRef.current = true;
      stop();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [refresh]);

  // health() on tab-open — drives the deployment banner honestly (no retry loop).
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const h = await leanzeroLinkHealth();
        if (!cancelled) setHealth(h);
      } catch {
        if (!cancelled) setHealth(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const connected = linkState?.auth.state === 'connected';
  useEffect(() => {
    if (!connected) return undefined;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [connected]);

  const requestCode = useCallback(
    async (email: string): Promise<boolean> => {
      setAuthError(null);
      setSubmitting(true);
      try {
        const result = await leanzeroLinkRequestCode(email);
        if (disposedRef.current) return true;
        const expiresAt = new Date(Date.now() + result.expiresInSeconds * 1000).toISOString();
        setLinkState((prev) => ({
          auth: { state: 'codeSent', email: result.email, expiresAt },
          mesh: prev?.mesh,
          nodeCount: prev?.nodeCount ?? 0,
          lastError: undefined,
        }));
        void refresh();
        return true;
      } catch (e) {
        if (!disposedRef.current) setAuthError(linkBannerText(e));
        return false;
      } finally {
        if (!disposedRef.current) setSubmitting(false);
      }
    },
    [refresh]
  );

  const verify = useCallback(
    async (email: string, code: string): Promise<void> => {
      setAuthError(null);
      setSubmitting(true);
      try {
        const result = await leanzeroLinkVerify(email, code);
        if (disposedRef.current) return;
        setAudienceSyncFailed(result.audienceSync === 'failed');
        setConnectError(null);
        setLinkState((prev) => ({
          auth: { state: 'loggedIn', email: result.email },
          mesh: prev?.mesh,
          nodeCount: prev?.nodeCount ?? 0,
          lastError: undefined,
        }));
        void refresh();
      } catch (e) {
        if (!disposedRef.current) setAuthError(linkBannerText(e));
      } finally {
        if (!disposedRef.current) setSubmitting(false);
      }
    },
    [refresh]
  );

  const connect = useCallback(async (): Promise<void> => {
    setConnectError(null);
    setConnecting(true);
    setLinkState((prev) =>
      prev && 'email' in prev.auth
        ? { ...prev, auth: { state: 'connecting', email: prev.auth.email } }
        : prev
    );
    try {
      const next = await leanzeroLinkConnect();
      if (disposedRef.current) return;
      setLinkState(next);
      void refresh();
    } catch (e) {
      if (!disposedRef.current) {
        setConnectError(linkBannerText(e));
        void refresh();
      }
    } finally {
      if (!disposedRef.current) setConnecting(false);
    }
  }, [refresh]);

  const doLogout = useCallback(async () => {
    setLoggingOut(true);
    try {
      const next = await leanzeroLinkLogout(wipe);
      if (disposedRef.current) return;
      setLinkState(next);
      setNodes(null);
      setAudienceSyncFailed(false);
      setConnectError(null);
      setAuthError(null);
    } catch (e) {
      if (!disposedRef.current) setStatusError(linkErrorText(e));
    } finally {
      if (!disposedRef.current) {
        setLoggingOut(false);
        setLogoutOpen(false);
        setWipe(false);
      }
    }
  }, [wipe]);

  const deployBanner = useMemo(() => {
    if (!health) return null;
    if (!health.capabilities.mesh) {
      return 'This LeanZero Link deployment has no mesh configured.';
    }
    if (!health.capabilities.mail) {
      return 'This LeanZero Link deployment has no email sign-in configured.';
    }
    return null;
  }, [health]);

  const auth = linkState?.auth ?? null;

  return (
    <div className="flex flex-col gap-4 pb-8">
      {deployBanner && (
        <ToneBanner tone="warn" label="Deployment" text={deployBanner} testId="link-deploy-banner" />
      )}
      {statusError && auth == null && (
        <ToneBanner tone="err" label="Link status" text={statusError} />
      )}

      {auth == null && !statusError ? (
        <div className={cx('flex items-center justify-center gap-2 py-16', TYPE.bodyMuted)}>
          <Loader2 className="size-4 animate-spin text-lz-accent" />
          Checking your LeanZero Link status…
        </div>
      ) : null}

      {(auth?.state === 'loggedOut' || auth?.state === 'codeSent' || (auth == null && statusError)) && (
        <LoginCard
          auth={
            auth?.state === 'codeSent'
              ? auth
              : ({ state: 'loggedOut' } as Extract<AuthState, { state: 'loggedOut' }>)
          }
          submitting={submitting}
          error={authError}
          onRequestCode={requestCode}
          onVerify={verify}
        />
      )}

      {auth?.state === 'loggedIn' && (
        <ConnectCard
          email={auth.email}
          connecting={connecting}
          error={connectError ?? linkState?.lastError ?? null}
          audienceSyncFailed={audienceSyncFailed}
          onConnect={() => void connect()}
        />
      )}

      {auth?.state === 'connecting' && <ConnectingCard email={auth.email} />}

      {auth?.state === 'connected' && linkState && (
        <ConnectedView
          email={auth.email}
          linkState={linkState}
          nodes={nodes}
          now={now}
          stale={staleFailures >= STALE_POLL_THRESHOLD}
          onLogout={() => setLogoutOpen(true)}
        />
      )}

      <ConfirmationModal
        isOpen={logoutOpen}
        title="Log out of LeanZero Link"
        message="This disconnects from the mesh and clears the stored identity on this Mac."
        detail={
          <WipeCheckbox
            checked={wipe}
            onChange={setWipe}
            label="Also wipe local mesh state (slower to sign back in)"
          />
        }
        confirmLabel="Log out"
        confirmVariant="destructive"
        isSubmitting={loggingOut}
        onConfirm={() => void doLogout()}
        onCancel={() => {
          setLogoutOpen(false);
          setWipe(false);
        }}
      />
    </div>
  );
};

export default LeanZeroLinkSection;
