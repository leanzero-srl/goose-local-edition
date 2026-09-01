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
import {
  Button,
  SectionHeader,
  StatusDot,
  FOCUS,
  MOTION,
  RADIUS,
  ROW,
  SURFACE,
  TNUM,
  TONE_FILL,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';
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
  currentSession: {
    id: 'projectsSection.currentSession',
    defaultMessage: 'Current session',
  },
});

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

// The tree registers, composed from the Studio tokens (ui/desktop/DESIGN.md). One dense 32px row
// for parents and leaves alike; hover is a solid step to surface-2; the current session is a 2px
// inset accent ring (a fill would hide its dot and meta).
const treeRowClass = cx(
  'flex w-full items-center gap-2 px-2 text-left',
  ROW.dense,
  RADIUS.control,
  MOTION,
  FOCUS
);
const treeParentClass = cx(treeRowClass, 'min-w-0 flex-1', SURFACE.hover);
const treeStateRowClass = cx('flex items-center px-2', ROW.dense, TYPE.meta);

/**
 * Children of an expanded row sit beside a 1px hairline guide (bg-lz-border, structural — it is
 * a separate element under the parent's chevron, never a border-left on the rows).
 */
const TreeChildren: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <div className="flex">
    <span
      aria-hidden
      data-testid="tree-guide"
      className={cx('ml-4 w-px shrink-0 self-stretch', 'bg-lz-border')}
    />
    <div className="flex min-w-0 flex-1 flex-col gap-px pl-2">{children}</div>
  </div>
);

// Row actions stay out of the way until the row is hovered or holds focus. Visibility, not
// opacity (opacity utilities are banned as faded colour); group-focus-within keeps them reachable
// by keyboard — focusing the row's own button reveals them for the next Tab.
const rowActionClass = (shown: boolean) =>
  shown ? 'visible' : 'invisible group-hover:visible group-focus-within:visible';

// Custom portaled context menu on the one overlay elevation; never a native menu. Remove has an
// in-menu confirm step so a single click can't drop a project, and the confirm label says out
// loud that removal touches the registry only.
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
  const itemBase = cx(
    'flex w-full items-center gap-2 px-3 text-left text-lz-body',
    ROW.dense,
    MOTION,
    FOCUS
  );
  const itemCls = cx(itemBase, 'text-lz-ink', SURFACE.hover);

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
        data-testid="project-context-menu"
        className={cx('fixed z-[200] py-1', SURFACE.overlay)}
        style={{ left, top, minWidth: MENU_W }}
        onClick={(e) => e.stopPropagation()}
      >
        <button className={itemCls} onClick={onNewSession}>
          <MessageSquarePlus className="size-3.5 text-lz-ink-3" />{' '}
          {intl.formatMessage(i18n.newSessionHere)}
        </button>
        <button className={itemCls} onClick={onReveal}>
          <FolderOpen className="size-3.5 text-lz-ink-3" />{' '}
          {intl.formatMessage(i18n.revealInFinder)}
        </button>
        <button className={itemCls} onClick={onCopyPath}>
          <Copy className="size-3.5 text-lz-ink-3" /> {intl.formatMessage(i18n.copyPath)}
        </button>
        <div className={cx('my-1 border-t', SURFACE.hairline)} />
        {confirming ? (
          <button className={cx(itemBase, TONE_FILL.err, 'hover:bg-lz-err')} onClick={onRemove}>
            <Check className="size-3.5" strokeWidth={3} /> {intl.formatMessage(i18n.confirmRemove)}
          </button>
        ) : (
          <button
            className={cx(itemBase, 'text-lz-err', SURFACE.hover)}
            onClick={() => setConfirming(true)}
          >
            <X className="size-3.5" /> {intl.formatMessage(i18n.removeFromProjects)}
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
  const intl = useIntl();
  const when = timeAgo(sessionActivityAt(session));
  const name = displaySessionListName(session.name);
  return (
    <button
      onClick={onClick}
      title={name}
      aria-current={active ? 'true' : undefined}
      className={cx(treeRowClass, active ? SURFACE.selectedRing : SURFACE.hover)}
    >
      <span className="flex-1 truncate text-lz-body text-lz-ink">{name}</span>
      {active && <StatusDot tone="accent" label={intl.formatMessage(i18n.currentSession)} />}
      {when ? <span className={cx('shrink-0', TYPE.meta, TNUM)}>{when}</span> : null}
    </button>
  );
};

interface ProjectRowProps {
  project: ProjectEntry;
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
        className="group relative flex items-center gap-px pr-1"
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        <button
          onClick={onToggle}
          aria-expanded={expanded}
          title={project.path}
          className={treeParentClass}
        >
          {expanded ? (
            <ChevronDown className="size-3.5 shrink-0 text-lz-ink-3" />
          ) : (
            <ChevronRight className="size-3.5 shrink-0 text-lz-ink-3" />
          )}
          {expanded ? (
            <FolderOpen className="size-4 shrink-0 text-lz-ink-2" />
          ) : (
            <Folder className="size-4 shrink-0 text-lz-ink-2" />
          )}
          <span className={cx('truncate text-lz-body text-lz-ink', WEIGHT.medium)}>{name}</span>
        </button>

        <Button
          variant="ghost"
          size="sm"
          icon={<Plus />}
          onClick={(e) => {
            e.stopPropagation();
            onNewSession();
          }}
          aria-label={`${intl.formatMessage(i18n.newSessionHere)} — ${name}`}
          title={intl.formatMessage(i18n.newSessionHere)}
          className={rowActionClass(false)}
        />
        <Button
          variant="ghost"
          size="sm"
          icon={<MoreVertical />}
          onClick={(e) => {
            e.stopPropagation();
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            setMenu({ x: r.right - 4, y: r.bottom + 2 });
          }}
          aria-label={intl.formatMessage(i18n.moreActions)}
          title={intl.formatMessage(i18n.moreActions)}
          className={rowActionClass(menu != null)}
        />

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
        <TreeChildren>
          {state?.error ? (
            <div className={cx('flex items-center gap-2 px-2', ROW.dense)}>
              <span className="text-lz-meta text-lz-err">
                {intl.formatMessage(i18n.sessionsFailed)}
              </span>
              <Button variant="ghost" size="sm" onClick={onRetry}>
                {intl.formatMessage(i18n.retry)}
              </Button>
            </div>
          ) : !state || (state.loading && !state.loaded) ? (
            <div className={treeStateRowClass}>{intl.formatMessage(i18n.loadingSessions)}</div>
          ) : state.sessions.length === 0 ? (
            <div className={treeStateRowClass}>{intl.formatMessage(i18n.noSessionsYet)}</div>
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
                <Button
                  variant="ghost"
                  size="sm"
                  className="self-start"
                  onClick={() => onLoadMore(state.nextCursor as string)}
                >
                  {intl.formatMessage(i18n.moreSessions)}
                </Button>
              ) : null}
              {state.loading ? (
                <div className={treeStateRowClass}>{intl.formatMessage(i18n.loadingSessions)}</div>
              ) : null}
            </>
          )}
        </TreeChildren>
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
    <div className={cx('flex min-h-0 flex-col', className)}>
      <SectionHeader
        title={intl.formatMessage(i18n.projects)}
        count={projects.length}
        className="px-4"
        right={
          <Button
            variant="ghost"
            size="sm"
            icon={<Plus className="text-lz-accent" strokeWidth={2.5} />}
            onClick={() => void handleAddProject()}
            aria-label={intl.formatMessage(i18n.addProject)}
            title={intl.formatMessage(i18n.addProject)}
          />
        }
      />

      <div className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto px-2 pb-2">
        {projects.length === 0 ? (
          <div className={cx('px-2 py-2', TYPE.bodyMuted)}>
            {intl.formatMessage(i18n.emptyState)}
          </div>
        ) : (
          projects.map((project) => (
            <ProjectRow
              key={project.path}
              project={project}
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
              className={cx(treeRowClass, SURFACE.hover)}
            >
              {unfiledOpen ? (
                <ChevronDown className="size-3.5 shrink-0 text-lz-ink-3" />
              ) : (
                <ChevronRight className="size-3.5 shrink-0 text-lz-ink-3" />
              )}
              <Inbox className="size-4 shrink-0 text-lz-ink-2" />
              <span className={cx('truncate text-lz-body text-lz-ink', WEIGHT.medium)}>
                {intl.formatMessage(i18n.unfiled)}
              </span>
              <span className={cx('ml-auto', TYPE.meta, TNUM)}>{unfiledSessions.length}</span>
            </button>
            {unfiledOpen && (
              <TreeChildren>
                {unfiledSessions.map((session) => (
                  <SessionLeafRow
                    key={session.id}
                    session={session}
                    active={session.id === activeSessionId}
                    onClick={() => handleSessionClick(session.id)}
                  />
                ))}
              </TreeChildren>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
