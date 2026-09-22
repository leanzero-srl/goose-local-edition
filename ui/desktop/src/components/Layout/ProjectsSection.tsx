import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  ChevronDown,
  ChevronRight,
  Copy,
  Folder,
  FolderOpen,
  GitFork,
  MessageSquarePlus,
  MoreVertical,
  Pencil,
  Plus,
  Sparkles,
  Trash2,
  X,
} from 'lucide-react';
import { toast } from 'react-toastify';
import { useConfig } from '../ConfigContext';
import { useNavigation } from '../../hooks/useNavigation';
import { useNavigationSessions } from '../../hooks/useNavigationSessions';
import { startNewSession, displaySessionListName } from '../../sessions';
import {
  acpDeleteSession,
  acpForkSession,
  acpListSessions,
  acpRenameSession,
  type SessionListItem,
} from '../../acp/sessions';
import { AppEvents } from '../../constants/events';
import {
  chooseAndAddProject,
  removeProjectAndBroadcast,
  type ProjectsChangedDetail,
} from '../../utils/addProjectFlow';
import { sessionActivityAt } from '../../utils/dateUtils';
import type { ProjectEntry } from '../../utils/projectDirs';
import {
  Button,
  SectionHeader,
  StatusDot,
  RADIUS,
  ROW,
  SURFACE,
  TNUM,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';
import {
  TreeChildren,
  TreeContextMenu,
  rowActionClass,
  timeAgo,
  treeParentClass,
  treeRowClass,
  treeStateRowClass,
  TREE_PREVIEW_COUNT,
} from './tree';
import { defineMessages, useIntl } from '../../i18n';
import { RenameDialog } from './RenameDialog';
import { useStartChatAbout } from './useStartChatAbout';

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
    defaultMessage:
      'Your sessions appear here under the folder they ran in. Start one, or add a folder.',
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
  showMore: {
    id: 'projectsSection.showMore',
    defaultMessage: 'Show more',
  },
  showLess: {
    id: 'projectsSection.showLess',
    defaultMessage: 'Show less',
  },
  noFolder: {
    id: 'projectsSection.noFolder',
    defaultMessage: 'No folder',
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
  openSession: { id: 'projectsSection.openSession', defaultMessage: 'Open' },
  renameSession: { id: 'projectsSection.renameSession', defaultMessage: 'Rename' },
  renameTitle: { id: 'projectsSection.renameTitle', defaultMessage: 'Rename session' },
  renameLabel: { id: 'projectsSection.renameLabel', defaultMessage: 'Name' },
  save: { id: 'projectsSection.save', defaultMessage: 'Save' },
  cancel: { id: 'projectsSection.cancel', defaultMessage: 'Cancel' },
  renameFailed: {
    id: 'projectsSection.renameFailed',
    defaultMessage: 'Could not rename the session',
  },
  forkSession: { id: 'projectsSection.forkSession', defaultMessage: 'Fork session' },
  forkFailed: { id: 'projectsSection.forkFailed', defaultMessage: 'Could not fork the session' },
  askAboutSession: {
    id: 'projectsSection.askAboutSession',
    defaultMessage: 'Start an AI session about this session',
  },
  deleteSession: { id: 'projectsSection.deleteSession', defaultMessage: 'Delete session' },
  confirmDeleteSession: {
    id: 'projectsSection.confirmDeleteSession',
    defaultMessage: 'Confirm delete (cannot be undone)',
  },
  deleteFailed: {
    id: 'projectsSection.deleteFailed',
    defaultMessage: 'Could not delete the session',
  },
});

/** What is asked of the model when a session is opened as a new chat about it. */
export function askAboutSessionPrompt(session: SessionListItem): string {
  const name = displaySessionListName(session.name);
  return [
    `I want to work from my earlier goose session "${name}" (session id ${session.id}, working directory ${session.workingDir}).`,
    'Read that session\'s conversation first (goose sessions are stored in its session database; use the session tools if you have them, otherwise ask me to paste the part that matters), then help me continue it, redo part of it, or turn what it learned into a skill or memory.',
    'Ask me what I want before you write anything.',
  ].join('\n');
}

/** Trailing-slash-insensitive normalization so membership tests mirror the server's exact-match cwd filter. */
export function normalizeDirPath(dir: string): string {
  const trimmed = dir.replace(/\/+$/, '');
  return trimmed.length > 0 ? trimmed : '/';
}

/** How many sessions a folder shows before "Show more" — the sidebar's density, not a storage cap. */
export const PREVIEW_COUNT = TREE_PREVIEW_COUNT;
/** How many of the newest folders start open. */
export const DEFAULT_OPEN_FOLDERS = 3;

export interface DerivedProject {
  /** The normalized working directory; the grouping key and the cwd filter for paging. */
  path: string;
  name: string;
  /** Sessions known from the recent list, newest first. */
  sessions: SessionListItem[];
  /** In the user's folder registry (added with "+"); such a folder stays listed with no sessions. */
  registered: boolean;
  lastActivity: number;
}

/**
 * Projects are DERIVED from where sessions ran — one folder per distinct working directory, its
 * sessions under it, newest folder first — the way ChatGPT Codex groups work. The "+" registry
 * only adds empty folders the user wants to start from; a folder with sessions needs no registry.
 * There is no "Unfiled": every session has a directory, so every session has a folder.
 */
export function deriveProjects(
  sessions: readonly SessionListItem[],
  registry: readonly ProjectEntry[]
): DerivedProject[] {
  const byPath = new Map<string, DerivedProject>();
  const activityOf = (s: SessionListItem) => Date.parse(sessionActivityAt(s) ?? '') || 0;
  for (const session of sessions) {
    const path = normalizeDirPath(session.workingDir ?? '');
    const at = activityOf(session);
    const cur = byPath.get(path);
    if (cur) {
      cur.sessions.push(session);
      cur.lastActivity = Math.max(cur.lastActivity, at);
    } else {
      byPath.set(path, {
        path,
        name: folderName(path),
        sessions: [session],
        registered: false,
        lastActivity: at,
      });
    }
  }
  for (const entry of registry) {
    const path = normalizeDirPath(entry.path);
    const cur = byPath.get(path);
    if (cur) {
      cur.registered = true;
    } else {
      byPath.set(path, {
        path,
        name: folderName(path),
        sessions: [],
        registered: true,
        lastActivity: 0,
      });
    }
  }
  const projects = [...byPath.values()];
  for (const project of projects) {
    project.sessions.sort((a, b) => activityOf(b) - activityOf(a));
  }
  projects.sort((a, b) => b.lastActivity - a.lastActivity || a.name.localeCompare(b.name));
  return projects;
}

/** Last path segment of a directory — the display name of a project. */
export function folderName(dir: string): string {
  const trimmed = dir.replace(/\/+$/, '');
  const seg = trimmed.split('/').filter(Boolean).pop();
  return seg ?? trimmed;
}

/** Sessions PAGED from the server beyond the recent list, per folder; `expandedAll` once the
 *  user asked for more than the preview. */
export interface ProjectSessionsState {
  sessions: SessionListItem[];
  nextCursor: string | null;
  loading: boolean;
  loaded: boolean;
  error: boolean;
}

const SessionLeafRow: React.FC<{
  session: SessionListItem;
  active: boolean;
  onClick: () => void;
  onRenamed: (name: string) => void;
  onDeleted: () => void;
  onForked: (newSessionId: string) => void;
  onAsk: () => void;
}> = ({ session, active, onClick, onRenamed, onDeleted, onForked, onAsk }) => {
  const intl = useIntl();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [renameError, setRenameError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const when = timeAgo(sessionActivityAt(session));
  const name = displaySessionListName(session.name);

  const rename = async (value: string) => {
    setBusy(true);
    setRenameError(null);
    try {
      await acpRenameSession(session.id, value);
      window.dispatchEvent(
        new CustomEvent(AppEvents.SESSION_RENAMED, {
          detail: { sessionId: session.id, newName: value, userInitiated: true },
        })
      );
      onRenamed(value);
      setRenaming(false);
    } catch (error) {
      setRenameError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    try {
      await acpDeleteSession(session.id);
      window.dispatchEvent(
        new CustomEvent(AppEvents.SESSION_DELETED, { detail: { sessionId: session.id } })
      );
      onDeleted();
    } catch (error) {
      console.error('Failed to delete session:', error);
      toast.error(intl.formatMessage(i18n.deleteFailed));
    }
  };

  const fork = async () => {
    try {
      const id = await acpForkSession(session.id);
      onForked(id);
    } catch (error) {
      console.error('Failed to fork session:', error);
      toast.error(intl.formatMessage(i18n.forkFailed));
    }
  };

  return (
    <div
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      <button
        onClick={onClick}
        title={name}
        aria-current={active ? 'true' : undefined}
        data-testid={`session-row-${session.id}`}
        className={cx(treeRowClass, active ? SURFACE.selectedRing : SURFACE.hover)}
      >
        <span className="flex-1 truncate text-lz-body text-lz-ink">{name}</span>
        {active && <StatusDot tone="accent" label={intl.formatMessage(i18n.currentSession)} />}
        {when ? <span className={cx('shrink-0', TYPE.meta, TNUM)}>{when}</span> : null}
      </button>
      {menu && (
        <TreeContextMenu
          x={menu.x}
          y={menu.y}
          testId="session-context-menu"
          onClose={() => setMenu(null)}
          items={[
            {
              key: 'open',
              label: intl.formatMessage(i18n.openSession),
              icon: <MessageSquarePlus />,
              onClick: () => {
                setMenu(null);
                onClick();
              },
            },
            {
              key: 'rename',
              label: intl.formatMessage(i18n.renameSession),
              icon: <Pencil />,
              onClick: () => {
                setMenu(null);
                setRenaming(true);
              },
            },
            {
              key: 'fork',
              label: intl.formatMessage(i18n.forkSession),
              icon: <GitFork />,
              onClick: () => {
                setMenu(null);
                void fork();
              },
            },
            {
              key: 'ask',
              label: intl.formatMessage(i18n.askAboutSession),
              icon: <Sparkles />,
              onClick: () => {
                setMenu(null);
                onAsk();
              },
            },
            {
              key: 'delete',
              label: intl.formatMessage(i18n.deleteSession),
              icon: <Trash2 />,
              danger: true,
              separator: true,
              confirmLabel: intl.formatMessage(i18n.confirmDeleteSession),
              onClick: () => {
                setMenu(null);
                void remove();
              },
            },
          ]}
        />
      )}
      {renaming && (
        <RenameDialog
          title={intl.formatMessage(i18n.renameTitle)}
          label={intl.formatMessage(i18n.renameLabel)}
          initial={name}
          saveLabel={intl.formatMessage(i18n.save)}
          cancelLabel={intl.formatMessage(i18n.cancel)}
          busy={busy}
          error={renameError}
          onSave={(value) => void rename(value)}
          onCancel={() => setRenaming(false)}
        />
      )}
    </div>
  );
};

interface ProjectRowProps {
  project: DerivedProject;
  expanded: boolean;
  /** Sessions beyond the recent list, paged from the server (after "Show more"). */
  state: ProjectSessionsState | undefined;
  showAll: boolean;
  activeSessionId?: string;
  onToggle: () => void;
  onNewSession: () => void;
  onRemove?: () => void;
  onOpenSession: (sessionId: string) => void;
  onAskSession: (session: SessionListItem) => void;
  onShowMore: () => void;
  onShowLess: () => void;
  onRetry: () => void;
}

const ProjectRow: React.FC<ProjectRowProps> = ({
  project,
  expanded,
  state,
  showAll,
  activeSessionId,
  onToggle,
  onNewSession,
  onRemove,
  onOpenSession,
  onAskSession,
  onShowMore,
  onShowLess,
  onRetry,
}) => {
  const intl = useIntl();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const name = project.name || intl.formatMessage(i18n.noFolder);

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

  const remove = useMemo(
    () =>
      onRemove
        ? () => {
            setMenu(null);
            onRemove();
          }
        : undefined,
    [onRemove]
  );

  // The recent list first, then whatever the server paged beyond it; one row per session.
  const known = useMemo(() => {
    const seen = new Set(project.sessions.map((s) => s.id));
    const paged = (state?.sessions ?? []).filter((s) => !seen.has(s.id));
    return [...project.sessions, ...paged];
  }, [project.sessions, state?.sessions]);
  const shown = showAll ? known : known.slice(0, PREVIEW_COUNT);
  const hiddenKnown = known.length - shown.length;
  // The server may hold sessions older than the recent list; it is asked only once the folder is
  // at least a full preview (a two-session folder is not hiding anything).
  const canPage = known.length >= PREVIEW_COUNT && !state?.loaded;
  const moreAvailable = hiddenKnown > 0 || canPage || !!state?.nextCursor;

  return (
    <div data-testid={`project-row-${project.path}`}>
      <div
        className="group relative flex items-center"
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
          {!expanded && known.length > 0 && (
            <span className={cx('ml-auto', TYPE.meta, TNUM)}>{known.length}</span>
          )}
        </button>

        {/* The actions float over the row's right edge so the folder name keeps the full width;
            they surface on hover/focus on the surface-2 step so the name under them stays legible. */}
        <div
          className={cx(
            'absolute right-1 top-1/2 flex -translate-y-1/2 items-center gap-px pl-1',
            RADIUS.control,
            SURFACE.inset,
            rowActionClass(menu != null)
          )}
        >
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
        </div>

        {menu && (
          <TreeContextMenu
            x={menu.x}
            y={menu.y}
            testId="project-context-menu"
            onClose={() => setMenu(null)}
            items={[
              {
                key: 'new',
                label: intl.formatMessage(i18n.newSessionHere),
                icon: <MessageSquarePlus />,
                onClick: newSession,
              },
              {
                key: 'reveal',
                label: intl.formatMessage(i18n.revealInFinder),
                icon: <FolderOpen />,
                onClick: reveal,
              },
              {
                key: 'copy',
                label: intl.formatMessage(i18n.copyPath),
                icon: <Copy />,
                onClick: copyPath,
              },
              ...(remove
                ? [
                    {
                      key: 'remove',
                      label: intl.formatMessage(i18n.removeFromProjects),
                      icon: <X />,
                      onClick: remove,
                      danger: true,
                      separator: true,
                      confirmLabel: intl.formatMessage(i18n.confirmRemove),
                    },
                  ]
                : []),
            ]}
          />
        )}
      </div>

      {expanded && (
        <TreeChildren>
          {shown.map((session) => (
            <SessionLeafRow
              key={session.id}
              session={session}
              active={session.id === activeSessionId}
              onClick={() => onOpenSession(session.id)}
              onRenamed={() => undefined}
              onDeleted={() => undefined}
              onForked={(id) => onOpenSession(id)}
              onAsk={() => onAskSession(session)}
            />
          ))}
          {known.length === 0 && !state?.loading && !state?.error ? (
            <div className={treeStateRowClass}>{intl.formatMessage(i18n.noSessionsYet)}</div>
          ) : null}
          {state?.error ? (
            <div className={cx('flex items-center gap-2 px-2', ROW.dense)}>
              <span className="text-lz-meta text-lz-err">
                {intl.formatMessage(i18n.sessionsFailed)}
              </span>
              <Button variant="ghost" size="sm" onClick={onRetry}>
                {intl.formatMessage(i18n.retry)}
              </Button>
            </div>
          ) : state?.loading ? (
            <div className={treeStateRowClass}>{intl.formatMessage(i18n.loadingSessions)}</div>
          ) : moreAvailable ? (
            <Button variant="ghost" size="sm" className="self-start" onClick={onShowMore}>
              {intl.formatMessage(i18n.showMore)}
            </Button>
          ) : showAll && known.length > PREVIEW_COUNT ? (
            <Button variant="ghost" size="sm" className="self-start" onClick={onShowLess}>
              {intl.formatMessage(i18n.showLess)}
            </Button>
          ) : null}
        </TreeChildren>
      )}
    </div>
  );
};

/**
 * The Projects tree: folders DERIVED from where sessions ran (deriveProjects), each expanded to its
 * sessions — the recent list first, then the server's cwd-filtered pages behind "Show more". The
 * "+" registry only adds an empty folder to start from. "New session here" inherits the folder
 * through the ordinary startNewSession path.
 */
export const ProjectsSection: React.FC<{ className?: string }> = ({ className }) => {
  const intl = useIntl();
  const setView = useNavigation();
  const { extensionsList } = useConfig();
  const { recentSessions, activeSessionId, fetchSessions, handleSessionClick } =
    useNavigationSessions();
  const startChat = useStartChatAbout();

  const [registry, setRegistry] = useState<ProjectEntry[]>([]);
  // Folders the user flipped; the default is open for the newest few and the one holding the open
  // session, closed for the rest (a 30-folder history is a wall when every folder starts open).
  const [toggled, setToggled] = useState<ReadonlySet<string>>(new Set());
  const [showAll, setShowAll] = useState<ReadonlySet<string>>(new Set());
  const [sessionsByProject, setSessionsByProject] = useState<Record<string, ProjectSessionsState>>(
    {}
  );

  useEffect(() => {
    window.electron
      .listProjects()
      .then(setRegistry)
      .catch((error) => console.error('Failed to load projects:', error));
    void fetchSessions();
  }, [fetchSessions]);

  const projects = useMemo(
    () => deriveProjects(recentSessions, registry),
    [recentSessions, registry]
  );

  const loadProjectSessions = useCallback(async (projectPath: string, cursor: string | null) => {
    setSessionsByProject((prev) => ({
      ...prev,
      [projectPath]: {
        sessions: prev[projectPath]?.sessions ?? [],
        nextCursor: prev[projectPath]?.nextCursor ?? null,
        loading: true,
        loaded: prev[projectPath]?.loaded ?? false,
        error: false,
      },
    }));
    try {
      const page = await acpListSessions(cursor, { cwd: projectPath });
      setSessionsByProject((prev) => {
        const cur = prev[projectPath]?.sessions ?? [];
        const fresh = page.sessions.filter((s) => !cur.some((x) => x.id === s.id));
        return {
          ...prev,
          [projectPath]: {
            sessions: cursor ? [...cur, ...fresh] : [...fresh],
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
          nextCursor: prev[projectPath]?.nextCursor ?? null,
          loading: false,
          loaded: prev[projectPath]?.loaded ?? false,
          error: true,
        },
      }));
    }
  }, []);

  const toggleProject = useCallback((projectPath: string) => {
    setToggled((prev) => {
      const next = new Set(prev);
      if (next.has(projectPath)) next.delete(projectPath);
      else next.add(projectPath);
      return next;
    });
  }, []);
  const isExpanded = useCallback(
    (project: DerivedProject, index: number) => {
      const defaultOpen =
        index < DEFAULT_OPEN_FOLDERS ||
        (activeSessionId != null && project.sessions.some((s) => s.id === activeSessionId));
      return toggled.has(project.path) ? !defaultOpen : defaultOpen;
    },
    [toggled, activeSessionId]
  );

  // "Show more" first reveals what the recent list already knows, then pages the server (the
  // exact cwd filter) until it runs out; "Show less" folds back to the preview.
  const showMore = useCallback(
    (project: DerivedProject) => {
      const state = sessionsByProject[project.path];
      if (!showAll.has(project.path)) {
        setShowAll((prev) => new Set(prev).add(project.path));
        if (project.sessions.length <= PREVIEW_COUNT && !state?.loaded) {
          void loadProjectSessions(project.path, null);
        }
        return;
      }
      if (!state?.loaded) {
        void loadProjectSessions(project.path, null);
      } else if (state.nextCursor) {
        void loadProjectSessions(project.path, state.nextCursor);
      }
    },
    [sessionsByProject, showAll, loadProjectSessions]
  );

  const showLess = useCallback((projectPath: string) => {
    setShowAll((prev) => {
      const next = new Set(prev);
      next.delete(projectPath);
      return next;
    });
  }, []);

  // ONE add-folder path (shared with the home landing): the flow broadcasts PROJECTS_CHANGED and
  // the listener below applies it — registry update, the new folder open — whichever surface ran
  // the picker.
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
      setRegistry(detail.projects);
      for (const p of detail.added) {
        // a freshly added folder opens regardless of its position
        setToggled((prev) => {
          const next = new Set(prev);
          next.add(`open:${normalizeDirPath(p.path)}`);
          return next;
        });
      }
    };
    window.addEventListener(AppEvents.PROJECTS_CHANGED, onProjectsChanged);
    return () => window.removeEventListener(AppEvents.PROJECTS_CHANGED, onProjectsChanged);
  }, []);

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

  // The recent list (the hook) already follows create/rename/delete; the paged extras follow
  // deletes and renames here so a folded-out folder never shows a ghost.
  useEffect(() => {
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
    window.addEventListener(AppEvents.SESSION_DELETED, onDeleted);
    window.addEventListener(AppEvents.SESSION_RENAMED, onRenamed);
    return () => {
      window.removeEventListener(AppEvents.SESSION_DELETED, onDeleted);
      window.removeEventListener(AppEvents.SESSION_RENAMED, onRenamed);
    };
  }, []);

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

      <div className="flex flex-col gap-px px-2 pb-2">
        {projects.length === 0 ? (
          <div className={cx('px-2 py-2', TYPE.bodyMuted)}>
            {intl.formatMessage(i18n.emptyState)}
          </div>
        ) : (
          projects.map((project, index) => (
            <ProjectRow
              key={project.path}
              project={project}
              expanded={toggled.has(`open:${project.path}`) || isExpanded(project, index)}
              state={sessionsByProject[project.path]}
              showAll={showAll.has(project.path)}
              activeSessionId={activeSessionId}
              onToggle={() => toggleProject(project.path)}
              onNewSession={() => void handleNewSession(project.path)}
              onRemove={
                project.registered && project.sessions.length === 0
                  ? () => void handleRemoveProject(project.path)
                  : undefined
              }
              onOpenSession={handleSessionClick}
              onAskSession={(session) => void startChat(askAboutSessionPrompt(session))}
              onShowMore={() => showMore(project)}
              onShowLess={() => showLess(project.path)}
              onRetry={() =>
                void loadProjectSessions(
                  project.path,
                  sessionsByProject[project.path]?.nextCursor ?? null
                )
              }
            />
          ))
        )}
      </div>
    </div>
  );
};
