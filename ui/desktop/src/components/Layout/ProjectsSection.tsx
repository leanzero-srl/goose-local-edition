import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { createPortal } from 'react-dom';
import {
  ChevronDown,
  ChevronRight,
  Check,
  Copy,
  Folder,
  FolderOpen,
  Inbox,
  MessageSquarePlus,
  MoreVertical,
  Plus,
  X,
} from 'lucide-react';
import { toast } from 'react-toastify';
import { useConfig } from '../ConfigContext';
import { useNavigation } from '../../hooks/useNavigation';
import { useNavigationSessions, sessionToListItem } from '../../hooks/useNavigationSessions';
import { startNewSession, displaySessionListName } from '../../sessions';
import { acpListSessions, type SessionListItem } from '../../acp/sessions';
import { AppEvents } from '../../constants/events';
import {
  chooseAndAddProject,
  removeProjectAndBroadcast,
  type ProjectsChangedDetail,
} from '../../utils/addProjectFlow';
import { sessionActivityAt } from '../../utils/dateUtils';
import type { ProjectEntry } from '../../utils/projectDirs';
import type { Session } from '../../types/session';
import { cn } from '../../utils';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  projects: {
    id: 'projectsSection.projects',
    defaultMessage: 'Projects',
  },
  addProject: {
    id: 'projectsSection.addProject',
    defaultMessage: 'Add a project folder',
  },
  emptyState: {
    id: 'projectsSection.emptyState',
    defaultMessage: 'Add a project folder to scope your sessions',
  },
  newSessionHere: {
    id: 'projectsSection.newSessionHere',
    defaultMessage: 'New session here',
  },
  revealInFinder: {
    id: 'projectsSection.revealInFinder',
    defaultMessage: 'Reveal in Finder',
  },
  copyPath: {
    id: 'projectsSection.copyPath',
    defaultMessage: 'Copy path',
  },
  pathCopied: {
    id: 'projectsSection.pathCopied',
    defaultMessage: 'Project path copied',
  },
  removeFromProjects: {
    id: 'projectsSection.removeFromProjects',
    defaultMessage: 'Remove from projects',
  },
  confirmRemove: {
    id: 'projectsSection.confirmRemove',
    defaultMessage: 'Confirm remove (keeps files & sessions)',
  },
  noSessionsYet: {
    id: 'projectsSection.noSessionsYet',
    defaultMessage: 'No sessions yet',
  },
  loadingSessions: {
    id: 'projectsSection.loadingSessions',
    defaultMessage: 'Loading sessions…',
  },
  sessionsFailed: {
    id: 'projectsSection.sessionsFailed',
    defaultMessage: "Couldn't load sessions",
  },
  retry: {
    id: 'projectsSection.retry',
    defaultMessage: 'Retry',
  },
  moreSessions: {
    id: 'projectsSection.moreSessions',
    defaultMessage: 'More sessions…',
  },
  unfiled: {
    id: 'projectsSection.unfiled',
    defaultMessage: 'Unfiled',
  },
  moreActions: {
    id: 'projectsSection.moreActions',
    defaultMessage: 'Project actions',
  },
  sessionFailed: {
    id: 'projectsSection.sessionFailed',
    defaultMessage: 'Could not start a session',
  },
  removeFailed: {
    id: 'projectsSection.removeFailed',
    defaultMessage: 'Could not update projects',
  },
  untitledSession: {
    id: 'projectsSection.untitledSession',
    defaultMessage: 'Untitled session',
  },
});

// Solid saturated hues (never tints) — each project gets its own, keyed by registry position, so a
// multi-project sidebar reads as a rainbow of distinct folders, matching the benchmark tile language.
const PROJECT_HUES = [
  '#2e8bff',
  '#6a5cff',
  '#17c4c4',
  '#f5a623',
  '#e84393',
  '#2ecc71',
  '#ff6b2c',
  '#b04adf',
] as const;
const ERROR_RED = '#ff3b30';
const UNFILED_GRAY = '#8a8a8a';

/** Trailing-slash-insensitive normalization so membership tests mirror the server's exact-match cwd filter. */
export function normalizeDirPath(dir: string): string {
  const trimmed = dir.replace(/\/+$/, '');
  return trimmed.length > 0 ? trimmed : '/';
}

/**
 * A session is UNFILED when its workingDir exactly matches no registered project. Exact match on
 * purpose: the server's cwd filter is exact, so a session in a project's SUBdirectory would never
 * appear under the project row — calling it "filed" here would make it vanish from the sidebar.
 */
export function isUnfiledSession(
  workingDir: string | undefined,
  projectPaths: ReadonlySet<string>
): boolean {
  if (!workingDir) return true;
  return !projectPaths.has(normalizeDirPath(workingDir));
}

/** Last path segment of a directory — the display name of a project. */
export function folderName(dir: string): string {
  const trimmed = dir.replace(/\/+$/, '');
  const seg = trimmed.split('/').filter(Boolean).pop();
  return seg ?? trimmed;
}

/** Compact "time since last activity". */
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

export interface ProjectSessionsState {
  sessions: SessionListItem[];
  nextCursor: string | null;
  loading: boolean;
  loaded: boolean;
  error: boolean;
}

// Custom portaled context menu — solid, sharp-cornered, full-bordered; never a native menu.
// Remove has an in-menu confirm step so a single click can't drop a project, and the confirm label
// says out loud that removal touches the registry only.
const ProjectContextMenu: React.FC<{
  x: number;
  y: number;
  onNewSession: () => void;
  onReveal: () => void;
  onCopyPath: () => void;
  onRemove: () => void;
  onClose: () => void;
}> = ({ x, y, onNewSession, onReveal, onCopyPath, onRemove, onClose }) => {
  const intl = useIntl();
  const [confirming, setConfirming] = useState(false);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const MENU_W = 240;
  const left = Math.min(x, window.innerWidth - MENU_W - 8);
  const top = Math.min(y, window.innerHeight - 180);
  const itemCls =
    'w-full text-left px-3 py-1.5 text-xs text-text-primary hover:bg-background-secondary flex items-center gap-2';

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
        <button className={itemCls} onClick={onNewSession}>
          <MessageSquarePlus size={13} /> {intl.formatMessage(i18n.newSessionHere)}
        </button>
        <button className={itemCls} onClick={onReveal}>
          <FolderOpen size={13} /> {intl.formatMessage(i18n.revealInFinder)}
        </button>
        <button className={itemCls} onClick={onCopyPath}>
          <Copy size={13} /> {intl.formatMessage(i18n.copyPath)}
        </button>
        <div className="my-1 border-t border-border-secondary" />
        {confirming ? (
          <button
            className="w-full text-left px-3 py-1.5 text-xs flex items-center gap-2 text-white"
            style={{ backgroundColor: ERROR_RED }}
            onClick={onRemove}
          >
            <Check size={13} strokeWidth={3} /> {intl.formatMessage(i18n.confirmRemove)}
          </button>
        ) : (
          <button
            className={`${itemCls} hover:!bg-transparent`}
            style={{ color: ERROR_RED }}
            onClick={() => setConfirming(true)}
          >
            <X size={13} /> {intl.formatMessage(i18n.removeFromProjects)}
          </button>
        )}
      </div>
    </>,
    document.body
  );
};

const SessionLeafRow: React.FC<{
  session: SessionListItem;
  active: boolean;
  onClick: () => void;
}> = ({ session, active, onClick }) => {
  const when = timeAgo(sessionActivityAt(session));
  const name = displaySessionListName(session.name);
  return (
    <button
      onClick={onClick}
      title={name}
      aria-current={active ? 'true' : undefined}
      className={cn(
        'w-full flex items-center gap-2 px-2 py-1.5 text-xs text-left transition-colors',
        active ? 'bg-background-tertiary text-text-primary' : 'hover:bg-background-tertiary/50'
      )}
      style={{ borderRadius: 4 }}
    >
      <span className="flex-1 truncate text-text-primary">{name}</span>
      {when ? (
        <span className="shrink-0 tabular-nums text-[11px] text-text-secondary">{when}</span>
      ) : null}
    </button>
  );
};

interface ProjectRowProps {
  project: ProjectEntry;
  hue: string;
  expanded: boolean;
  state: ProjectSessionsState | undefined;
  activeSessionId?: string;
  onToggle: () => void;
  onNewSession: () => void;
  onRemove: () => void;
  onOpenSession: (sessionId: string) => void;
  onLoadMore: (cursor: string) => void;
  onRetry: () => void;
}

const ProjectRow: React.FC<ProjectRowProps> = ({
  project,
  hue,
  expanded,
  state,
  activeSessionId,
  onToggle,
  onNewSession,
  onRemove,
  onOpenSession,
  onLoadMore,
  onRetry,
}) => {
  const intl = useIntl();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const name = folderName(project.path);

  const reveal = useCallback(() => {
    setMenu(null);
    void window.electron.revealInFinder(project.path);
  }, [project.path]);

  const copyPath = useCallback(() => {
    setMenu(null);
    void navigator.clipboard.writeText(project.path);
    toast.success(intl.formatMessage(i18n.pathCopied));
  }, [project.path, intl]);

  const newSession = useCallback(() => {
    setMenu(null);
    onNewSession();
  }, [onNewSession]);

  const remove = useCallback(() => {
    setMenu(null);
    onRemove();
  }, [onRemove]);

  return (
    <div>
      <div
        className="group relative flex items-center gap-1 pr-1.5"
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        <button
          onClick={onToggle}
          aria-expanded={expanded}
          title={project.path}
          className="flex-1 min-w-0 flex items-center gap-2 px-2 py-1.5 text-sm hover:bg-background-tertiary/50 transition-colors"
          style={{ borderRadius: 4 }}
        >
          {expanded ? (
            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
          )}
          <span
            className="inline-flex items-center justify-center shrink-0"
            style={{ width: 18, height: 18, borderRadius: 3, backgroundColor: hue }}
          >
            <Folder className="h-3 w-3" strokeWidth={2.5} style={{ color: '#0b0b0b' }} />
          </span>
          <span className="truncate font-medium text-text-primary">{name}</span>
        </button>

        <button
          onClick={(e) => {
            e.stopPropagation();
            onNewSession();
          }}
          aria-label={`${intl.formatMessage(i18n.newSessionHere)} — ${name}`}
          title={intl.formatMessage(i18n.newSessionHere)}
          className={cn(
            'shrink-0 text-text-secondary hover:text-text-primary transition-opacity',
            'opacity-0 group-hover:opacity-100 focus-visible:opacity-100'
          )}
        >
          <Plus className="h-4 w-4" />
        </button>
        <button
          onClick={(e) => {
            e.stopPropagation();
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            setMenu({ x: r.right - 4, y: r.bottom + 2 });
          }}
          aria-label={intl.formatMessage(i18n.moreActions)}
          title={intl.formatMessage(i18n.moreActions)}
          className={cn(
            'shrink-0 text-text-secondary hover:text-text-primary transition-opacity',
            menu ? 'opacity-100' : 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100'
          )}
        >
          <MoreVertical className="h-4 w-4" />
        </button>

        {menu && (
          <ProjectContextMenu
            x={menu.x}
            y={menu.y}
            onNewSession={newSession}
            onReveal={reveal}
            onCopyPath={copyPath}
            onRemove={remove}
            onClose={() => setMenu(null)}
          />
        )}
      </div>

      {expanded && (
        <div className="ml-6 mr-1 flex flex-col gap-0.5">
          {state?.error ? (
            <div className="px-2 py-1.5 text-xs flex items-center gap-2">
              <span style={{ color: ERROR_RED }}>{intl.formatMessage(i18n.sessionsFailed)}</span>
              <button onClick={onRetry} className="font-semibold text-text-primary hover:underline">
                {intl.formatMessage(i18n.retry)}
              </button>
            </div>
          ) : !state || (state.loading && !state.loaded) ? (
            <div className="px-2 py-1.5 text-xs text-text-secondary">
              {intl.formatMessage(i18n.loadingSessions)}
            </div>
          ) : state.sessions.length === 0 ? (
            <div className="px-2 py-1.5 text-xs text-text-secondary">
              {intl.formatMessage(i18n.noSessionsYet)}
            </div>
          ) : (
            <>
              {state.sessions.map((session) => (
                <SessionLeafRow
                  key={session.id}
                  session={session}
                  active={session.id === activeSessionId}
                  onClick={() => onOpenSession(session.id)}
                />
              ))}
              {state.nextCursor && !state.loading ? (
                <button
                  onClick={() => onLoadMore(state.nextCursor as string)}
                  className="px-2 py-1 text-xs text-left font-semibold hover:underline"
                  style={{ color: hue }}
                >
                  {intl.formatMessage(i18n.moreSessions)}
                </button>
              ) : null}
              {state.loading ? (
                <div className="px-2 py-1 text-xs text-text-secondary">
                  {intl.formatMessage(i18n.loadingSessions)}
                </div>
              ) : null}
            </>
          )}
        </div>
      )}
    </div>
  );
};

/**
 * ChatGPT-style Projects tree: a user-curated registry of local folders; each project expands to
 * that folder's historical sessions, fetched with the SERVER-SIDE cwd filter (exact, paginated).
 * "New session here" inherits the project's directory through the ordinary startNewSession path.
 * Below the projects, "Unfiled" lists recent sessions whose workingDir matches no project — a nav
 * nicety over the recent-sessions hook, not paginated truth.
 */
export const ProjectsSection: React.FC<{ className?: string }> = ({ className }) => {
  const intl = useIntl();
  const setView = useNavigation();
  const { extensionsList } = useConfig();
  const { recentSessions, activeSessionId, fetchSessions, handleSessionClick } =
    useNavigationSessions();

  const [projects, setProjects] = useState<ProjectEntry[]>([]);
  const [expandedPaths, setExpandedPaths] = useState<ReadonlySet<string>>(new Set());
  const [unfiledOpen, setUnfiledOpen] = useState(false);
  const [sessionsByProject, setSessionsByProject] = useState<Record<string, ProjectSessionsState>>(
    {}
  );

  useEffect(() => {
    window.electron
      .listProjects()
      .then(setProjects)
      .catch((error) => console.error('Failed to load projects:', error));
    void fetchSessions();
  }, [fetchSessions]);

  const loadProjectSessions = useCallback(async (projectPath: string) => {
    setSessionsByProject((prev) => ({
      ...prev,
      [projectPath]: {
        sessions: prev[projectPath]?.sessions ?? [],
        nextCursor: null,
        loading: true,
        loaded: prev[projectPath]?.loaded ?? false,
        error: false,
      },
    }));
    try {
      const page = await acpListSessions(null, { cwd: projectPath });
      setSessionsByProject((prev) => {
        // Keep locally-known zero-message sessions the server doesn't list yet (it only returns
        // sessions with messages) — a just-created chat must not vanish from its project.
        const localEmpty = (prev[projectPath]?.sessions ?? []).filter(
          (s) => s.messageCount === 0 && !page.sessions.some((listed) => listed.id === s.id)
        );
        return {
          ...prev,
          [projectPath]: {
            sessions: [...localEmpty, ...page.sessions],
            nextCursor: page.nextCursor,
            loading: false,
            loaded: true,
            error: false,
          },
        };
      });
    } catch (error) {
      console.error('Failed to load project sessions:', error);
      setSessionsByProject((prev) => ({
        ...prev,
        [projectPath]: {
          sessions: prev[projectPath]?.sessions ?? [],
          nextCursor: null,
          loading: false,
          loaded: true,
          error: true,
        },
      }));
    }
  }, []);

  const loadMoreProjectSessions = useCallback(async (projectPath: string, cursor: string) => {
    setSessionsByProject((prev) => {
      const cur = prev[projectPath];
      if (!cur) return prev;
      return { ...prev, [projectPath]: { ...cur, loading: true } };
    });
    try {
      const page = await acpListSessions(cursor, { cwd: projectPath });
      setSessionsByProject((prev) => {
        const cur = prev[projectPath];
        if (!cur) return prev;
        const fresh = page.sessions.filter((s) => !cur.sessions.some((x) => x.id === s.id));
        return {
          ...prev,
          [projectPath]: {
            sessions: [...cur.sessions, ...fresh],
            nextCursor: page.nextCursor,
            loading: false,
            loaded: true,
            error: false,
          },
        };
      });
    } catch (error) {
      console.error('Failed to load more project sessions:', error);
      setSessionsByProject((prev) => {
        const cur = prev[projectPath];
        if (!cur) return prev;
        return { ...prev, [projectPath]: { ...cur, loading: false, error: true } };
      });
    }
  }, []);

  const toggleProject = useCallback(
    (projectPath: string) => {
      const isOpen = expandedPaths.has(projectPath);
      setExpandedPaths((prev) => {
        const next = new Set(prev);
        if (isOpen) {
          next.delete(projectPath);
        } else {
          next.add(projectPath);
        }
        return next;
      });
      if (!isOpen) {
        void loadProjectSessions(projectPath);
      }
    },
    [expandedPaths, loadProjectSessions]
  );

  // ONE add-project path (shared with the home landing): the flow broadcasts PROJECTS_CHANGED
  // and the listener below applies it — registry update, expand the new project, load its
  // sessions — no matter which surface ran the picker.
  const handleAddProject = useCallback(async () => {
    try {
      await chooseAndAddProject();
    } catch (error) {
      console.error('Failed to add project:', error);
      toast.error(intl.formatMessage(i18n.removeFailed));
    }
  }, [intl]);

  const handleRemoveProject = useCallback(
    async (projectPath: string) => {
      try {
        await removeProjectAndBroadcast(projectPath);
      } catch (error) {
        console.error('Failed to remove project:', error);
        toast.error(intl.formatMessage(i18n.removeFailed));
      }
    },
    [intl]
  );

  useEffect(() => {
    const onProjectsChanged = (event: Event) => {
      const detail = (event as CustomEvent<ProjectsChangedDetail>).detail;
      if (!detail) return;
      setProjects(detail.projects);
      for (const p of detail.added) {
        setExpandedPaths((prev) => new Set(prev).add(p.path));
        void loadProjectSessions(p.path);
      }
    };
    window.addEventListener(AppEvents.PROJECTS_CHANGED, onProjectsChanged);
    return () => window.removeEventListener(AppEvents.PROJECTS_CHANGED, onProjectsChanged);
  }, [loadProjectSessions]);

  const handleNewSession = useCallback(
    async (projectPath: string) => {
      try {
        await startNewSession(undefined, setView, projectPath, {
          allExtensions: extensionsList,
        });
      } catch (error) {
        console.error('Failed to start project session:', error);
        toast.error(intl.formatMessage(i18n.sessionFailed));
      }
    },
    [setView, extensionsList, intl]
  );

  // Keep expanded project lists truthful against the session event stream: a created session is
  // prepended under its project immediately; deletes remove; renames update in place.
  useEffect(() => {
    const onCreated = (event: Event) => {
      const { session } = (event as CustomEvent<{ session?: Session }>).detail || {};
      if (!session) return;
      const dir = normalizeDirPath(session.working_dir ?? '');
      setSessionsByProject((prev) => {
        const key = Object.keys(prev).find((p) => normalizeDirPath(p) === dir);
        if (!key) return prev;
        const cur = prev[key];
        const item = sessionToListItem(session);
        if (cur.sessions.some((s) => s.id === item.id)) return prev;
        return { ...prev, [key]: { ...cur, sessions: [item, ...cur.sessions] } };
      });
    };

    const onDeleted = (event: Event) => {
      const { sessionId } = (event as CustomEvent<{ sessionId: string }>).detail;
      setSessionsByProject((prev) => {
        const next: Record<string, ProjectSessionsState> = {};
        for (const [key, cur] of Object.entries(prev)) {
          next[key] = { ...cur, sessions: cur.sessions.filter((s) => s.id !== sessionId) };
        }
        return next;
      });
    };

    const onRenamed = (event: Event) => {
      const { sessionId, newName } = (event as CustomEvent<{ sessionId: string; newName: string }>)
        .detail;
      setSessionsByProject((prev) => {
        const next: Record<string, ProjectSessionsState> = {};
        for (const [key, cur] of Object.entries(prev)) {
          next[key] = {
            ...cur,
            sessions: cur.sessions.map((s) => (s.id === sessionId ? { ...s, name: newName } : s)),
          };
        }
        return next;
      });
    };

    window.addEventListener(AppEvents.SESSION_CREATED, onCreated);
    window.addEventListener(AppEvents.SESSION_DELETED, onDeleted);
    window.addEventListener(AppEvents.SESSION_RENAMED, onRenamed);
    return () => {
      window.removeEventListener(AppEvents.SESSION_CREATED, onCreated);
      window.removeEventListener(AppEvents.SESSION_DELETED, onDeleted);
      window.removeEventListener(AppEvents.SESSION_RENAMED, onRenamed);
    };
  }, []);

  const projectPathSet = useMemo(
    () => new Set(projects.map((p) => normalizeDirPath(p.path))),
    [projects]
  );
  const unfiledSessions = useMemo(
    () => recentSessions.filter((s) => isUnfiledSession(s.workingDir, projectPathSet)),
    [recentSessions, projectPathSet]
  );

  return (
    <div className={cn('flex flex-col min-h-0', className)}>
      <div className="flex items-center justify-between pl-4 pr-3 py-1">
        <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
          {intl.formatMessage(i18n.projects)}
        </span>
        <button
          onClick={() => void handleAddProject()}
          aria-label={intl.formatMessage(i18n.addProject)}
          title={intl.formatMessage(i18n.addProject)}
          className="inline-flex items-center justify-center hover:opacity-80 transition-opacity"
          style={{ width: 18, height: 18, borderRadius: 3, backgroundColor: PROJECT_HUES[0] }}
        >
          <Plus className="h-3 w-3" strokeWidth={3} style={{ color: '#0b0b0b' }} />
        </button>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto px-2 pb-2 flex flex-col gap-0.5">
        {projects.length === 0 ? (
          <div className="px-2 py-2 text-xs text-text-secondary">
            {intl.formatMessage(i18n.emptyState)}
          </div>
        ) : (
          projects.map((project, index) => (
            <ProjectRow
              key={project.path}
              project={project}
              hue={PROJECT_HUES[index % PROJECT_HUES.length]}
              expanded={expandedPaths.has(project.path)}
              state={sessionsByProject[project.path]}
              activeSessionId={activeSessionId}
              onToggle={() => toggleProject(project.path)}
              onNewSession={() => void handleNewSession(project.path)}
              onRemove={() => void handleRemoveProject(project.path)}
              onOpenSession={handleSessionClick}
              onLoadMore={(cursor) => void loadMoreProjectSessions(project.path, cursor)}
              onRetry={() => void loadProjectSessions(project.path)}
            />
          ))
        )}

        {unfiledSessions.length > 0 && (
          <div className="mt-2">
            <button
              onClick={() => setUnfiledOpen((v) => !v)}
              aria-expanded={unfiledOpen}
              className="w-full flex items-center gap-2 px-2 py-1.5 text-sm hover:bg-background-tertiary/50 transition-colors"
              style={{ borderRadius: 4 }}
            >
              {unfiledOpen ? (
                <ChevronDown className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
              ) : (
                <ChevronRight className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
              )}
              <span
                className="inline-flex items-center justify-center shrink-0"
                style={{ width: 18, height: 18, borderRadius: 3, backgroundColor: UNFILED_GRAY }}
              >
                <Inbox className="h-3 w-3" strokeWidth={2.5} style={{ color: '#0b0b0b' }} />
              </span>
              <span className="truncate font-medium text-text-primary">
                {intl.formatMessage(i18n.unfiled)}
              </span>
              <span className="ml-auto text-[11px] tabular-nums text-text-secondary">
                {unfiledSessions.length}
              </span>
            </button>
            {unfiledOpen && (
              <div className="ml-6 mr-1 flex flex-col gap-0.5">
                {unfiledSessions.map((session) => (
                  <SessionLeafRow
                    key={session.id}
                    session={session}
                    active={session.id === activeSessionId}
                    onClick={() => handleSessionClick(session.id)}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
