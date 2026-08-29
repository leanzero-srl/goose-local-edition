import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useLocation, useNavigate } from 'react-router-dom';
import { useSwarmRun } from '../swarm/useSwarmRun';
import { useEdition } from '../../contexts/EditionContext';
import {
  ChevronDown,
  ChevronRight,
  Trash2,
  Check,
  Folder,
  FolderOpen,
  ExternalLink,
  MoreVertical,
  Copy,
  Pencil,
  Loader2,
  AlertCircle,
  FileText,
  Hammer,
  MessageSquare,
  Pause,
} from 'lucide-react';
import { motion } from 'framer-motion';
import { toast } from 'react-toastify';
import { useNavigationContext } from './NavigationContext';
import { useConfig } from '../ConfigContext';
import { useNavigationSessions } from '../../hooks/useNavigationSessions';
import {
  NAV_ITEMS,
  SETTINGS_NAV_ITEM,
  getNavItemLabel,
  type NavItem,
} from '../../hooks/useNavigationItems';
import { AppEvents } from '../../constants/events';
import { acpRenameSession, acpDeleteSession, type SessionListItem } from '../../acp/sessions';
import { sessionActivityAt } from '../../utils/dateUtils';
import { cn } from '../../utils';
import { defineMessages, useIntl } from '../../i18n';

type StreamState = 'idle' | 'loading' | 'streaming' | 'error';

interface SessionStatus {
  streamState: StreamState;
  hasUnreadActivity: boolean;
}

const i18n = defineMessages({
  chats: {
    id: 'navigationPanel.chats',
    defaultMessage: 'Chats',
  },
  noChats: {
    id: 'navigationPanel.noChats',
    defaultMessage: 'No recent chats',
  },
  untitledSession: {
    id: 'navigationPanel.untitledSession',
    defaultMessage: 'Untitled session',
  },
  deleteChat: {
    id: 'navigationPanel.deleteChat',
    defaultMessage: 'Delete chat',
  },
  confirmDelete: {
    id: 'navigationPanel.confirmDelete',
    defaultMessage: 'Confirm delete',
  },
  cancel: {
    id: 'navigationPanel.cancel',
    defaultMessage: 'Cancel',
  },
  chatDeleted: {
    id: 'navigationPanel.chatDeleted',
    defaultMessage: 'Chat deleted',
  },
  deleteFailed: {
    id: 'navigationPanel.deleteFailed',
    defaultMessage: 'Could not delete chat',
  },
  rename: {
    id: 'navigationPanel.rename',
    defaultMessage: 'Rename',
  },
  revealInFinder: {
    id: 'navigationPanel.revealInFinder',
    defaultMessage: 'Reveal folder in Finder',
  },
  copyPath: {
    id: 'navigationPanel.copyPath',
    defaultMessage: 'Copy folder path',
  },
  openNewWindow: {
    id: 'navigationPanel.openNewWindow',
    defaultMessage: 'Open in new window',
  },
  pathCopied: {
    id: 'navigationPanel.pathCopied',
    defaultMessage: 'Folder path copied',
  },
  moreActions: {
    id: 'navigationPanel.moreActions',
    defaultMessage: 'More actions',
  },
  kindBuild: {
    id: 'navigationPanel.kindBuild',
    defaultMessage: 'build',
  },
  kindRecipe: {
    id: 'navigationPanel.kindRecipe',
    defaultMessage: 'recipe',
  },
  streaming: {
    id: 'navigationPanel.streaming',
    defaultMessage: 'Working now',
  },
  hasError: {
    id: 'navigationPanel.hasError',
    defaultMessage: 'Ended with an error',
  },
  messagesLabel: {
    id: 'navigationPanel.messages',
    defaultMessage: '{count} msg',
  },
});

const navItemClass = (active: boolean) =>
  cn(
    'flex flex-row items-center gap-3 outline-none no-drag w-full',
    'rounded-full px-3 py-2 text-sm font-medium transition-colors',
    active
      ? 'bg-background-tertiary text-text-primary'
      : 'text-text-primary hover:bg-background-tertiary/60'
  );

interface NavRowProps {
  item: NavItem;
  active: boolean;
  onClick: () => void;
}

export const NavRow: React.FC<NavRowProps> = ({ item, active, onClick }) => {
  const intl = useIntl();
  const Icon = item.icon;
  return (
    <button
      onClick={onClick}
      className={navItemClass(active)}
      // The active view was styled and nothing more: bg-background-tertiary against a transparent
      // sibling, with no aria-current and no aria-selected anywhere in the panel. So the current view
      // was visible to a sighted mouse user and invisible to everything else — a screen reader, and any
      // automated check of "which view am I on". Measured live over CDP on #/benchmark: the Benchmark
      // row computed rgb(71,78,87) against rgba(0,0,0,0), and a sweep of all 19 nav controls found ZERO
      // with either attribute set.
      aria-current={active ? 'page' : undefined}
    >
      <Icon className="w-5 h-5 flex-shrink-0 text-text-secondary" />
      <span className="text-left flex-1 truncate">{getNavItemLabel(item, intl)}</span>
      {item.getTag && (
        <span className="text-xs font-mono text-text-secondary">{item.getTag()}</span>
      )}
    </button>
  );
};

// Status/kind palette — mirrors the swarm panel's STATUS_COLOR + FORMATION_RAMP so a session reads the same
// language as its build panel. Solid, saturated hues; the leading tile is a filled sharp square (never a rail).
const STATUS_COLOR = { running: '#f5a623', done: '#2ecc71', error: '#ff3b30' } as const;
const LOADING_BLUE = '#2e8bff';
// Same amber the swarm panel's Paused badge and pause button use — a held run must read identically
// wherever it appears, or the sidebar and the panel disagree about the same fact.
const PAUSED_AMBER = '#d97706';
const UNREAD_GREEN = '#2ecc71';
// Kind hues for an idle row (no live status): recipe = violet, build = teal, plain chat = neutral slate.
const KIND_COLOR = { recipe: '#6a5cff', build: '#17c4c4', chat: '#8a8a8a' } as const;

type SessionKind = 'recipe' | 'build' | 'chat';

/** How a row reads at a glance. `hasRecipe` is engine truth; the "build" bucket is a DISPLAY heuristic off the
 *  app's own auto-name ("Build <brief> — a"), never a verdict — it only picks the glyph, not a status. */
function sessionKind(session: SessionListItem): SessionKind {
  if (session.hasRecipe) return 'recipe';
  if (/^\s*build\b/i.test(session.name)) return 'build';
  return 'chat';
}

/** Last path segment of a working dir — the built-app / workspace folder. The primary disambiguator for the
 *  many identically-named build sessions ("Build logfold — a" ×15), which differ only by folder + time. */
function folderName(dir: string | undefined): string {
  if (!dir) return '';
  const trimmed = dir.replace(/\/+$/, '');
  const seg = trimmed.split('/').filter(Boolean).pop();
  return seg ?? trimmed;
}

/** Compact "time since last activity" — the second disambiguator for duplicate titles. */
function timeAgo(iso: string | undefined): string {
  if (!iso) return '';
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return '';
  const s = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (s < 45) return 'now';
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d}d ago`;
  const w = Math.round(d / 7);
  return w < 5 ? `${w}w ago` : `${Math.round(d / 30)}mo ago`;
}

// Custom right-click / kebab menu for a session row — solid, sharp-cornered, full-bordered, portaled at the
// cursor. NEVER a native menu (matches the swarm panel's ActivityContextMenu). Delete has an in-menu confirm
// step so a single click can't lose a chat.
const SessionContextMenu: React.FC<{
  x: number;
  y: number;
  hasFolder: boolean;
  onReveal: () => void;
  onCopyPath: () => void;
  onOpenNewWindow: () => void;
  onRename: () => void;
  onDelete: () => void;
  onClose: () => void;
}> = ({ x, y, hasFolder, onReveal, onCopyPath, onOpenNewWindow, onRename, onDelete, onClose }) => {
  const intl = useIntl();
  const [confirming, setConfirming] = useState(false);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const MENU_W = 220;
  const left = Math.min(x, window.innerWidth - MENU_W - 8);
  const top = Math.min(y, window.innerHeight - 240);
  const itemCls =
    'w-full text-left px-3 py-1.5 text-xs text-text-primary hover:bg-background-secondary flex items-center gap-2 disabled:opacity-40 disabled:hover:bg-transparent';

  return createPortal(
    <>
      <div
        className="fixed inset-0 z-[190]"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        className="fixed z-[200] bg-background-primary border border-border-primary shadow-lg py-1"
        style={{ left, top, minWidth: MENU_W, borderRadius: 3 }}
        onClick={(e) => e.stopPropagation()}
      >
        <button className={itemCls} onClick={onOpenNewWindow}>
          <ExternalLink size={13} /> {intl.formatMessage(i18n.openNewWindow)}
        </button>
        <button className={itemCls} onClick={onReveal} disabled={!hasFolder}>
          <FolderOpen size={13} /> {intl.formatMessage(i18n.revealInFinder)}
        </button>
        <button className={itemCls} onClick={onCopyPath} disabled={!hasFolder}>
          <Copy size={13} /> {intl.formatMessage(i18n.copyPath)}
        </button>
        <button className={itemCls} onClick={onRename}>
          <Pencil size={13} /> {intl.formatMessage(i18n.rename)}
        </button>
        <div className="my-1 border-t border-border-secondary" />
        {confirming ? (
          <button
            className="w-full text-left px-3 py-1.5 text-xs flex items-center gap-2 text-white"
            style={{ backgroundColor: STATUS_COLOR.error }}
            onClick={onDelete}
          >
            <Check size={13} strokeWidth={3} /> {intl.formatMessage(i18n.confirmDelete)}
          </button>
        ) : (
          <button
            className={`${itemCls} hover:!bg-transparent`}
            style={{ color: STATUS_COLOR.error }}
            onClick={() => setConfirming(true)}
          >
            <Trash2 size={13} /> {intl.formatMessage(i18n.deleteChat)}
          </button>
        )}
      </div>
    </>,
    document.body
  );
};

interface SessionRowProps {
  session: SessionListItem;
  active: boolean;
  status: SessionStatus | undefined;
  onClick: () => void;
  onRenamed: () => void;
}

const KIND_ICON: Record<SessionKind, typeof Hammer> = {
  recipe: FileText,
  build: Hammer,
  chat: MessageSquare,
};

const SessionRow: React.FC<SessionRowProps> = ({ session, active, status, onClick, onRenamed }) => {
  const intl = useIntl();
  const navigate = useNavigate();
  const [isEditing, setIsEditing] = useState(false);
  const [editValue, setEditValue] = useState(session.name);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const isStreaming = status?.streamState === 'streaming';
  const isLoading = status?.streamState === 'loading';
  const hasError = status?.streamState === 'error';
  const hasUnread = status?.hasUnreadActivity ?? false;

  const kind = sessionKind(session);
  const folder = folderName(session.workingDir);
  const when = timeAgo(sessionActivityAt(session));
  const KindIcon = KIND_ICON[kind];

  // The leading tile: live status colour wins (running/error/loading), else the kind hue. Solid saturated fill,
  // dark glyph — reads in both themes, exactly like the swarm FleetStrip node tiles.
  const streamLive = isStreaming || isLoading || hasError;
  // #137: streamState is the CHAT stream, and it stays 'streaming' through a swarm HOLD because the provider
  // call is still open — so this tile span a run where every fleet node was deliberately idle. Mihai read the
  // spinner as a hang and resumed a healthy run. Poll ONLY for a row that is already live (a conditional
  // ARGUMENT, not a conditional hook; an undefined dir makes useSwarmRun return EMPTY without polling), so
  // this stays one poller for the one running chat rather than one per row.
  const swarm = useSwarmRun(streamLive ? session.workingDir : undefined);
  const swarmHeld = swarm.held;
  const live = streamLive && !swarmHeld;
  const tileColor = hasError
    ? STATUS_COLOR.error
    : swarmHeld
      ? PAUSED_AMBER
      : isStreaming
        ? STATUS_COLOR.running
        : isLoading
          ? LOADING_BLUE
          : KIND_COLOR[kind];
  const TileGlyph = swarmHeld ? Pause : live ? (hasError ? AlertCircle : Loader2) : KindIcon;
  const tileTip = hasError
    ? intl.formatMessage(i18n.hasError)
    : swarmHeld
      ? 'Swarm paused — nothing is running until you resume'
      : isStreaming || isLoading
        ? intl.formatMessage(i18n.streaming)
      : kind === 'recipe'
        ? intl.formatMessage(i18n.kindRecipe)
        : kind === 'build'
          ? intl.formatMessage(i18n.kindBuild)
          : '';

  // A row only grows a second (meta) line when it actually has meta — a brand-new empty chat stays one line.
  const showMeta = !!folder || session.messageCount > 0;

  const commitRename = useCallback(async () => {
    const next = editValue.trim();
    setIsEditing(false);
    if (!next || next === session.name) return;
    try {
      await acpRenameSession(session.id, next);
      window.dispatchEvent(
        new CustomEvent(AppEvents.SESSION_RENAMED, {
          detail: { sessionId: session.id, newName: next, userInitiated: true },
        })
      );
      onRenamed();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : intl.formatMessage(i18n.deleteFailed));
      setEditValue(session.name);
    }
  }, [editValue, session.id, session.name, onRenamed, intl]);

  const startRename = useCallback(() => {
    if (isStreaming) return;
    setEditValue(session.name);
    setIsEditing(true);
    setMenu(null);
    requestAnimationFrame(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    });
  }, [isStreaming, session.name]);

  const doDelete = useCallback(async () => {
    setMenu(null);
    try {
      await acpDeleteSession(session.id);
      // Leaving the active session first stops the pair route re-adding it as a ghost via ?resumeSessionId.
      if (active) navigate('/');
      window.dispatchEvent(
        new CustomEvent(AppEvents.SESSION_DELETED, { detail: { sessionId: session.id } })
      );
      toast.success(intl.formatMessage(i18n.chatDeleted));
    } catch (err) {
      toast.error(err instanceof Error ? err.message : intl.formatMessage(i18n.deleteFailed));
    }
  }, [session.id, active, navigate, intl]);

  const openNewWindow = useCallback(() => {
    setMenu(null);
    window.electron.createChatWindow({
      dir: session.workingDir,
      resumeSessionId: session.id,
      viewType: 'pair',
    });
  }, [session.workingDir, session.id]);

  const revealFolder = useCallback(() => {
    setMenu(null);
    if (session.workingDir) void window.electron.revealInFinder(session.workingDir);
  }, [session.workingDir]);

  const copyPath = useCallback(() => {
    setMenu(null);
    if (!session.workingDir) return;
    void navigator.clipboard.writeText(session.workingDir);
    toast.success(intl.formatMessage(i18n.pathCopied));
  }, [session.workingDir, intl]);

  return (
    <div
      onClick={() => !isEditing && onClick()}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
      className={cn(
        'group relative flex items-start gap-2.5 px-2.5 py-2 cursor-pointer text-sm',
        'border border-transparent transition-colors',
        active
          ? 'bg-background-tertiary border-border-primary'
          : 'hover:bg-background-tertiary/50'
      )}
      style={{ borderRadius: 4 }}
    >
      <span
        title={tileTip}
        aria-label={tileTip}
        className="mt-0.5 inline-flex items-center justify-center shrink-0"
        style={{ width: 18, height: 18, borderRadius: 3, backgroundColor: tileColor }}
      >
        <TileGlyph
          className={cn('h-3 w-3', (isStreaming || isLoading) && 'animate-spin')}
          strokeWidth={2.5}
          style={{ color: '#0b0b0b' }}
        />
      </span>

      <div className="flex-1 min-w-0">
        {isEditing ? (
          <input
            ref={inputRef}
            type="text"
            value={editValue}
            maxLength={200}
            onChange={(e) => setEditValue(e.target.value)}
            onClick={(e) => e.stopPropagation()}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                void commitRename();
              } else if (e.key === 'Escape') {
                e.preventDefault();
                setEditValue(session.name);
                setIsEditing(false);
              }
            }}
            className="w-full bg-background-primary text-text-primary text-sm px-1.5 py-0.5 border border-border-active outline-none"
            style={{ borderRadius: 3 }}
          />
        ) : (
          <div
            className="truncate font-medium text-text-primary"
            title={session.name}
            onDoubleClick={(e) => {
              e.stopPropagation();
              startRename();
            }}
          >
            {session.name || intl.formatMessage(i18n.untitledSession)}
          </div>
        )}

        {showMeta && !isEditing && (
          <div className="mt-1 flex items-center gap-2 text-[11px] text-text-secondary min-w-0">
            {folder ? (
              <span
                className="inline-flex items-center gap-1 min-w-0 flex-1"
                title={session.workingDir}
              >
                <Folder className="h-2.5 w-2.5 shrink-0" />
                <span className="truncate font-mono">{folder}</span>
              </span>
            ) : null}
            {session.messageCount > 0 ? (
              <span
                className="shrink-0 inline-flex items-center gap-0.5 tabular-nums"
                title={intl.formatMessage(i18n.messagesLabel, { count: session.messageCount })}
              >
                <MessageSquare className="h-2.5 w-2.5" />
                {session.messageCount}
              </span>
            ) : null}
            {when ? <span className="shrink-0 ml-auto tabular-nums">{when}</span> : null}
          </div>
        )}
      </div>

      {hasUnread && !live ? (
        <span
          className="mt-1 shrink-0"
          aria-label="Has new activity"
          style={{ width: 7, height: 7, borderRadius: 2, backgroundColor: UNREAD_GREEN }}
        />
      ) : null}

      {!isEditing && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            setMenu({ x: r.right - 4, y: r.bottom + 2 });
          }}
          aria-label={intl.formatMessage(i18n.moreActions)}
          title={intl.formatMessage(i18n.moreActions)}
          className={cn(
            'shrink-0 -mr-0.5 text-text-secondary hover:text-text-primary transition-opacity',
            menu ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
          )}
        >
          <MoreVertical className="h-4 w-4" />
        </button>
      )}

      {menu && (
        <SessionContextMenu
          x={menu.x}
          y={menu.y}
          hasFolder={!!session.workingDir}
          onReveal={revealFolder}
          onCopyPath={copyPath}
          onOpenNewWindow={openNewWindow}
          onRename={startRename}
          onDelete={doDelete}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
};

export const Navigation: React.FC<{ className?: string }> = ({ className }) => {
  const intl = useIntl();
  const { isNavExpanded } = useNavigationContext();
  const location = useLocation();
  const { extensionsList } = useConfig();

  const appsExtensionEnabled = !!extensionsList?.find((ext) => ext.name === 'apps')?.enabled;
  const { isLocal } = useEdition();

  const visibleItems = useMemo<NavItem[]>(() => {
    return NAV_ITEMS.filter((item) => {
      if (item.path === '/apps') return appsExtensionEnabled;
      // Benchmark measures a local fleet, so it has no meaning in an upstream-flavoured build.
      if (item.path === '/benchmark') return isLocal;
      return true;
    });
  }, [appsExtensionEnabled, isLocal]);

  const isActive = useCallback((path: string) => location.pathname === path, [location.pathname]);

  const { recentSessions, activeSessionId, fetchSessions, handleNavClick, handleSessionClick } =
    useNavigationSessions();

  const [sessionStatuses, setSessionStatuses] = useState<Map<string, SessionStatus>>(new Map());

  useEffect(() => {
    const handleStatusUpdate = (event: Event) => {
      const { sessionId, streamState } = (event as CustomEvent).detail;
      setSessionStatuses((prev) => {
        const existing = prev.get(sessionId);
        const shouldMarkUnread = existing?.streamState === 'streaming' && streamState === 'idle';
        const next = new Map(prev);
        next.set(sessionId, {
          streamState,
          hasUnreadActivity: existing?.hasUnreadActivity || shouldMarkUnread,
        });
        return next;
      });
    };

    window.addEventListener(AppEvents.SESSION_STATUS_UPDATE, handleStatusUpdate);
    return () => window.removeEventListener(AppEvents.SESSION_STATUS_UPDATE, handleStatusUpdate);
  }, []);

  const clearUnread = useCallback((sessionId: string) => {
    setSessionStatuses((prev) => {
      const status = prev.get(sessionId);
      if (status?.hasUnreadActivity) {
        const next = new Map(prev);
        next.set(sessionId, { ...status, hasUnreadActivity: false });
        return next;
      }
      return prev;
    });
  }, []);

  const navFocusRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (isNavExpanded) {
      fetchSessions();
      requestAnimationFrame(() => navFocusRef.current?.focus());
    }
  }, [isNavExpanded, fetchSessions]);

  const [isChatsExpanded, setIsChatsExpanded] = useState(true);

  if (!isNavExpanded) return null;

  return (
    <motion.div
      ref={navFocusRef}
      tabIndex={-1}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.15 }}
      className={cn('bg-background-primary outline-none flex flex-col h-full', className)}
    >
      <div className="h-[48px] no-drag" />

      {/* Nav items */}
      <div className="px-2 flex flex-col gap-0.5">
        {visibleItems.map((item) => (
          <NavRow
            key={item.id}
            item={item}
            active={isActive(item.path)}
            onClick={() => handleNavClick(item.path)}
          />
        ))}
      </div>

      {/* Chats section — takes remaining vertical space */}
      <div className="flex-1 min-h-0 flex flex-col mt-3">
        <button
          onClick={() => setIsChatsExpanded((v) => !v)}
          className="flex items-center gap-1 px-4 py-1 text-xs font-semibold uppercase tracking-wider text-text-secondary hover:text-text-primary transition-colors self-start"
        >
          {isChatsExpanded ? (
            <ChevronDown className="w-3 h-3" />
          ) : (
            <ChevronRight className="w-3 h-3" />
          )}
          <span>{intl.formatMessage(i18n.chats)}</span>
        </button>
        {isChatsExpanded && (
          <div className="flex-1 min-h-0 overflow-y-auto px-2 pb-2 mt-1 flex flex-col gap-0.5">
            {recentSessions.length === 0 ? (
              <div className="px-3 py-2 text-xs text-text-secondary">
                {intl.formatMessage(i18n.noChats)}
              </div>
            ) : (
              recentSessions.map((session) => (
                <SessionRow
                  key={session.id}
                  session={session}
                  active={session.id === activeSessionId}
                  status={sessionStatuses.get(session.id)}
                  onClick={() => {
                    clearUnread(session.id);
                    handleSessionClick(session.id);
                  }}
                  onRenamed={fetchSessions}
                />
              ))
            )}
          </div>
        )}
      </div>

      {/* Settings pinned to bottom */}
      <div className="px-2 pt-2 pb-2 border-t border-border-secondary">
        <NavRow
          item={SETTINGS_NAV_ITEM}
          active={isActive(SETTINGS_NAV_ITEM.path)}
          onClick={() => handleNavClick(SETTINGS_NAV_ITEM.path)}
        />
      </div>
    </motion.div>
  );
};
