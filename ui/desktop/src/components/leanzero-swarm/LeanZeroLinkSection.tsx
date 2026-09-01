import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowLeft,
  Check,
  Laptop,
  Link2,
  Loader2,
  LogOut,
  Mail,
  RefreshCw,
  Wifi,
  WifiOff,
} from 'lucide-react';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { defineMessages, useIntl } from '../../i18n';
import {
  AZURE,
  GREEN,
  AMBER,
  RED,
  SLATE,
  INK_DARK,
  Chip,
  SolidBanner,
} from './primitives';
import {
  leanzeroLinkConnect,
  leanzeroLinkHealth,
  leanzeroLinkLogout,
  leanzeroLinkNodes,
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
  color: string;
  label: string;
  sessionId?: string;
}

function nodeStatusVisual(status: NodeStatus): StatusVisual {
  switch (status.type) {
    case 'Idle':
      return { color: GREEN, label: 'idle' };
    case 'Busy':
      return { color: AMBER, label: 'busy', sessionId: status.session_id };
    case 'Offline':
    default:
      return { color: SLATE, label: 'offline' };
  }
}

// ---------------------------------------------------------------------------
// Shared card chrome (benchmark register: full borders, solid fills, no rails/tints).
// Declared at module scope — never inside a render body.
// ---------------------------------------------------------------------------

function Card({ children }: { children: React.ReactNode }) {
  return <div className="overflow-hidden rounded border border-border-primary">{children}</div>;
}

function CardHeader({ label, right }: { label: string; right?: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-border-primary bg-background-secondary px-3 py-2">
      <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
        {label}
      </span>
      {right != null && <span className="ml-auto flex items-center gap-2">{right}</span>}
    </div>
  );
}

function StatusChip({ status }: { status: NodeStatus }) {
  const v = nodeStatusVisual(status);
  return (
    <Chip
      color={v.color}
      ink={v.color === AMBER ? INK_DARK : '#ffffff'}
      title={v.sessionId ? `busy on session ${v.sessionId}` : v.label}
    >
      {v.label}
      {v.sessionId ? ` · ${v.sessionId.slice(0, 8)}` : ''}
    </Chip>
  );
}

function DeviceRow({
  node,
  now,
  isSelf,
}: {
  node: NodeState;
  now: number;
  isSelf: boolean;
}) {
  return (
    <div
      className="flex flex-wrap items-center gap-x-4 gap-y-1 px-3 py-2.5"
      data-testid={isSelf ? 'link-self' : `link-peer-${node.node_id}`}
    >
      <Laptop className="h-4 w-4 shrink-0 text-text-secondary" />
      <span className="min-w-0 truncate text-sm font-bold text-text-primary">{node.hostname}</span>
      <StatusChip status={node.status} />
      <span className="font-mono text-xs text-text-secondary">
        {node.mesh_ip ?? 'no mesh IP'}
      </span>
      {node.sessions_active > 0 && (
        <span className="text-xs font-semibold" style={{ color: AZURE }}>
          {node.sessions_active} active
        </span>
      )}
      {!isSelf && (
        <span className="ml-auto text-xs text-text-secondary">
          {formatLastSeen(node.updated_at, now)}
        </span>
      )}
    </div>
  );
}

// Custom checkbox (no native input, per the design bans). Solid AZURE fill when checked.
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
      className="flex items-center gap-2 text-left text-sm text-text-primary"
      data-testid="link-wipe-checkbox"
    >
      <span
        className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${
          checked ? '' : 'border-border-primary'
        }`}
        style={checked ? { backgroundColor: AZURE, borderColor: AZURE } : undefined}
      >
        {checked && <Check className="h-3 w-3 text-white" />}
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
      <Card>
        <div className="flex flex-col items-center gap-1 border-b border-border-primary bg-background-secondary px-6 py-5 text-center">
          <span
            className="mb-1 inline-flex h-11 w-11 items-center justify-center rounded"
            style={{ backgroundColor: AZURE }}
          >
            <Link2 className="h-6 w-6 text-white" />
          </span>
          <h2 className="text-lg font-bold text-text-primary">LeanZero Link</h2>
          <p className="max-w-[42ch] text-sm text-text-secondary">
            {intl.formatMessage(i18n.tagline)}
          </p>
        </div>

        <div className="flex flex-col gap-4 px-6 py-6">
          {error && <SolidBanner color={RED} label="Sign-in" text={error} />}

          {!codeStage ? (
            <form
              className="flex flex-col gap-3"
              onSubmit={(e) => {
                e.preventDefault();
                void submitEmail();
              }}
            >
              <label
                htmlFor="link-email-input"
                className="text-xs font-semibold uppercase tracking-wider text-text-secondary"
              >
                Email
              </label>
              <Input
                id="link-email-input"
                data-testid="link-email-input"
                type="email"
                autoComplete="email"
                inputMode="email"
                placeholder="you@example.com"
                value={email}
                disabled={submitting}
                onChange={(e) => setEmail(e.target.value)}
              />
              <Button
                type="submit"
                disabled={submitting || email.trim() === ''}
                data-testid="link-send-code"
                className="w-full rounded font-bold text-white hover:opacity-90"
                style={{ backgroundColor: AZURE }}
              >
                {submitting ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Mail className="h-4 w-4" />
                )}
                Send code
              </Button>
            </form>
          ) : (
            <div className="flex flex-col gap-3">
              <div className="flex items-center justify-between gap-2">
                <span className="text-sm text-text-secondary">
                  Code sent to{' '}
                  <span className="font-semibold text-text-primary" data-testid="link-masked-email">
                    {maskEmail(codeEmail)}
                  </span>
                </span>
                <span
                  className="rounded px-2 py-0.5 text-sm font-bold tabular-nums"
                  style={{ backgroundColor: expired ? RED : AZURE, color: '#ffffff' }}
                  data-testid="link-countdown"
                  title={expired ? 'the code has expired' : 'time until the code expires'}
                >
                  {formatCountdown(remainingSecs)}
                </span>
              </div>
              <label
                htmlFor="link-code-input"
                className="text-xs font-semibold uppercase tracking-wider text-text-secondary"
              >
                6-digit code
              </label>
              <Input
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
                className="text-center font-mono text-2xl tracking-[0.5em]"
              />
              {expired && (
                <span className="text-xs font-semibold" style={{ color: RED }}>
                  This code has expired — send a new one.
                </span>
              )}
              <Button
                type="button"
                disabled={!canVerify}
                data-testid="link-verify"
                onClick={() => void onVerify(codeEmail, digits)}
                className="w-full rounded font-bold text-white hover:opacity-90"
                style={{ backgroundColor: AZURE }}
              >
                {submitting ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Check className="h-4 w-4" />
                )}
                Verify
              </Button>
              <div className="flex items-center justify-between gap-2 text-xs">
                <button
                  type="button"
                  data-testid="link-resend"
                  disabled={submitting}
                  onClick={() => void onRequestCode(codeEmail)}
                  className="flex items-center gap-1 font-semibold text-text-secondary hover:text-text-primary disabled:opacity-50"
                >
                  <RefreshCw className="h-3 w-3" />
                  Send a new code
                </button>
                <button
                  type="button"
                  data-testid="link-different-email"
                  disabled={submitting}
                  onClick={() => {
                    setBackToEmail(true);
                    setCode('');
                  }}
                  className="flex items-center gap-1 font-semibold text-text-secondary hover:text-text-primary disabled:opacity-50"
                >
                  <ArrowLeft className="h-3 w-3" />
                  Use a different email
                </button>
              </div>
            </div>
          )}
        </div>
      </Card>
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
      <Card>
        <CardHeader label="Signed in" />
        <div className="flex flex-col gap-4 px-6 py-6">
          <div className="flex items-center gap-3">
            <span
              className="inline-flex h-9 w-9 items-center justify-center rounded"
              style={{ backgroundColor: GREEN }}
            >
              <Mail className="h-5 w-5 text-white" />
            </span>
            <span className="min-w-0 truncate text-sm font-bold text-text-primary">{email}</span>
          </div>

          {audienceSyncFailed && (
            <div
              className="rounded px-3 py-2 text-xs font-semibold"
              style={{ backgroundColor: AMBER, color: INK_DARK }}
              data-testid="link-audience-note"
            >
              Signed in, but syncing your contact info to LeanZero didn&apos;t go through. Your
              account works — this is just the mailing audience.
            </div>
          )}

          {error && <SolidBanner color={RED} label="Connect failed" text={error} />}

          <p className="text-sm text-text-secondary">
            Bring this Mac onto your private mesh so your linked devices can see each other.
          </p>

          <Button
            type="button"
            disabled={connecting}
            data-testid="link-connect"
            onClick={onConnect}
            className="w-full rounded font-bold text-white hover:opacity-90"
            style={{ backgroundColor: AZURE }}
          >
            {connecting ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Link2 className="h-4 w-4" />
            )}
            {error ? 'Retry connect' : 'Connect to mesh'}
          </Button>
        </div>
      </Card>
    </div>
  );
}

function ConnectingCard({ email }: { email: string }) {
  return (
    <div className="mx-auto w-full max-w-md" data-testid="link-connecting">
      <Card>
        <CardHeader label="Connecting" />
        <div className="flex flex-col items-center gap-3 px-6 py-10 text-center">
          <Loader2 className="h-8 w-8 animate-spin" style={{ color: AZURE }} />
          <span className="text-sm font-bold text-text-primary">Joining your private mesh</span>
          <span className="text-xs text-text-secondary">{email}</span>
        </div>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Connected dashboard.
// ---------------------------------------------------------------------------

function ConnectedView({
  email,
  linkState,
  nodes,
  now,
  onLogout,
}: {
  email: string;
  linkState: LinkState;
  nodes: NodesResponse | null;
  now: number;
  onLogout: () => void;
}) {
  const mesh = linkState.mesh;
  const peers = nodes?.peers ?? [];

  return (
    <div className="flex flex-col gap-4 pb-8" data-testid="link-connected">
      <Card>
        <CardHeader
          label="Account"
          right={
            <Button
              type="button"
              size="sm"
              variant="outline"
              data-testid="link-logout"
              onClick={onLogout}
              className="rounded font-bold"
            >
              <LogOut className="h-3.5 w-3.5" />
              Log out / Switch account
            </Button>
          }
        />
        <div className="flex flex-wrap items-center gap-3 px-3 py-3">
          <span
            className="inline-flex h-8 w-8 items-center justify-center rounded"
            style={{ backgroundColor: GREEN }}
          >
            <Mail className="h-4 w-4 text-white" />
          </span>
          <span className="min-w-0 truncate text-sm font-bold text-text-primary">{email}</span>
          <span
            className="ml-auto flex items-center gap-1.5 text-xs font-semibold"
            data-testid="link-mesh-line"
            style={{ color: mesh?.online ? GREEN : SLATE }}
          >
            {mesh?.online ? <Wifi className="h-3.5 w-3.5" /> : <WifiOff className="h-3.5 w-3.5" />}
            mesh {mesh?.backendState ?? 'unknown'}
            {mesh?.online ? ' · online' : ' · offline'}
            {' · '}
            {linkState.nodeCount} node{linkState.nodeCount === 1 ? '' : 's'}
          </span>
        </div>
      </Card>

      {linkState.lastError && (
        <SolidBanner color={RED} label="Mesh" text={linkState.lastError} />
      )}

      <Card>
        <CardHeader label="This device" />
        {nodes?.self ? (
          <DeviceRow node={nodes.self} now={now} isSelf />
        ) : (
          <div className="flex items-center gap-2 px-3 py-3 text-sm text-text-secondary">
            <Loader2 className="h-4 w-4 animate-spin" />
            Reading this device&apos;s state…
          </div>
        )}
      </Card>

      <Card>
        <CardHeader
          label="Linked devices"
          right={
            <span className="text-xs font-bold tabular-nums" style={{ color: AZURE }}>
              {peers.length}
            </span>
          }
        />
        {peers.length === 0 ? (
          <div
            className="px-3 py-6 text-center text-sm text-text-secondary"
            data-testid="link-peers-empty"
          >
            No other devices linked yet — sign in on another Mac to see it here.
          </div>
        ) : (
          <div className="divide-y divide-border-primary" data-testid="link-peers">
            {peers.map((peer) => (
              <DeviceRow key={peer.node_id} node={peer} now={now} isSelf={false} />
            ))}
          </div>
        )}
      </Card>
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

  const disposedRef = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const next = await leanzeroLinkStatus();
      if (disposedRef.current) return;
      setLinkState(next);
      setStatusError(null);
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
      // Keep the last known state; surface the read failure as a truth line.
      setStatusError(linkErrorText(e));
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
        <div data-testid="link-deploy-banner">
          <SolidBanner color={AMBER} label="Deployment" text={deployBanner} />
        </div>
      )}
      {statusError && auth == null && (
        <SolidBanner color={RED} label="Link status" text={statusError} />
      )}

      {auth == null && !statusError ? (
        <div className="flex items-center justify-center gap-2 py-16 text-sm text-text-secondary">
          <Loader2 className="h-4 w-4 animate-spin" />
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
