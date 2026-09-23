import { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { FolderPlus, MessageSquare, Plus } from 'lucide-react';
import { toast } from 'react-toastify';
import { defineMessages, useIntl } from '../i18n';
import { AppEvents } from '../constants/events';
import { chooseAndAddProject, type ProjectsChangedDetail } from '../utils/addProjectFlow';
import type { ProjectEntry } from '../utils/projectDirs';
import { acpListRecentSessions, type SessionListItem } from '../acp/sessions';
import { displaySessionListName, startNewSession } from '../sessions';
import { sessionActivityAt } from '../utils/dateUtils';
import { useNavigation } from '../hooks/useNavigation';
import { MAX_RECENT_SESSIONS } from '../hooks/useNavigationSessions';
import { useConfig } from './ConfigContext';
import { deriveProjects, folderName, type DerivedProject } from './Layout/ProjectsSection';
import { timeAgo } from './Layout/tree';
import { deskHref, deskName } from './Layout/AgentWorkSection';
import { foldDesk, type AgentWorkRosterRow } from './agent-work/agentWorkModel';
import {
  Button,
  EmptyState,
  FOCUS,
  KeyValue,
  MOTION,
  PageHeader,
  Panel,
  SPACE,
  SURFACE,
  TNUM,
  TYPE,
  WEIGHT,
  cx,
} from './lz';
import { LEANZERO_MARK_VIEWBOX, LeanZeroMarkContent } from './icons/leanzeroMark';

const i18n = defineMessages({
  headline: {
    id: 'projectLanding.headline',
    defaultMessage: 'Start from a project',
  },
  emptyHint: {
    id: 'projectLanding.emptyHint',
    defaultMessage: 'Add a project folder, then start sessions from it in the sidebar.',
  },
  addProject: {
    id: 'projectLanding.addProject',
    defaultMessage: 'Add a project',
  },
  addFailed: {
    id: 'projectLanding.addFailed',
    defaultMessage: 'Could not add the project folder',
  },
  givesTitle: {
    id: 'projectLanding.givesTitle',
    defaultMessage: 'What a project gives you',
  },
  giveDirLabel: {
    id: 'projectLanding.giveDirLabel',
    defaultMessage: 'Working directory',
  },
  giveDirValue: {
    id: 'projectLanding.giveDirValue',
    defaultMessage: 'The project folder',
  },
  giveSessionsLabel: {
    id: 'projectLanding.giveSessionsLabel',
    defaultMessage: 'Sessions',
  },
  giveSessionsValue: {
    id: 'projectLanding.giveSessionsValue',
    defaultMessage: 'Listed under their project',
  },
  giveUnfiledLabel: {
    id: 'projectLanding.giveUnfiledLabel',
    defaultMessage: 'Any other folder',
  },
  giveUnfiledValue: {
    id: 'projectLanding.giveUnfiledValue',
    defaultMessage: 'Becomes a project the moment a session runs there',
  },
  continueTitle: {
    id: 'projectLanding.continueTitle',
    defaultMessage: 'Pick up where you left off',
  },
  continueSubtitle: {
    id: 'projectLanding.continueSubtitle',
    defaultMessage: 'Resume a recent session, answer what an agent is waiting on, or start fresh.',
  },
  newSessionIn: {
    id: 'projectLanding.newSessionIn',
    defaultMessage: 'New session in {project}',
  },
  startFailed: {
    id: 'projectLanding.startFailed',
    defaultMessage: 'Could not start a session',
  },
  recentTitle: {
    id: 'projectLanding.recentTitle',
    defaultMessage: 'Recent sessions',
  },
  noSessionsYet: {
    id: 'projectLanding.noSessionsYet',
    defaultMessage: 'No sessions yet. Start the first one in {project}.',
  },
  sessionsFailed: {
    id: 'projectLanding.sessionsFailed',
    defaultMessage: 'Could not load your sessions: {error}',
  },
  retry: {
    id: 'projectLanding.retry',
    defaultMessage: 'Retry',
  },
  needsTitle: {
    id: 'projectLanding.needsTitle',
    defaultMessage: 'Waiting on you',
  },
  openAsks: {
    id: 'projectLanding.openAsks',
    defaultMessage: 'Open questions: {count}',
  },
  pendingDrafts: {
    id: 'projectLanding.pendingDrafts',
    defaultMessage: 'Drafts to approve: {count}',
  },
  deskUnreadable: {
    id: 'projectLanding.deskUnreadable',
    defaultMessage: 'Could not read this desk: {error}',
  },
});

/** How many sessions the landing lists — the newest few, the rest live in the sidebar tree. */
export const LANDING_RECENT_COUNT = 8;

interface DeskNeed {
  dir: string;
  name: string;
  asks: number;
  drafts: number;
  error?: string;
}

/**
 * The desks that wait on the person: each rostered desk read once and folded through the desk
 * view's own fold (foldDesk → openAsks, pendingDrafts), so this list and the desk page cannot
 * disagree about what "waiting on you" means. A desk that cannot be read is listed with its error.
 */
async function readDeskNeeds(rows: AgentWorkRosterRow[]): Promise<DeskNeed[]> {
  const live = rows.filter((r) => r.exists);
  const out = await Promise.all(
    live.map(async (row): Promise<DeskNeed | null> => {
      try {
        const model = foldDesk(await window.electron.agentWorkRead(row.dir), Date.now());
        const asks = model?.openAsks.length ?? 0;
        const drafts = model?.pendingDrafts.length ?? 0;
        return asks + drafts > 0 ? { dir: row.dir, name: deskName(row), asks, drafts } : null;
      } catch (e) {
        return {
          dir: row.dir,
          name: deskName(row),
          asks: 0,
          drafts: 0,
          error: e instanceof Error ? e.message : String(e),
        };
      }
    })
  );
  return out.filter((d): d is DeskNeed => d != null);
}

const rowButtonClass = cx(
  'flex w-full min-w-0 items-center gap-3 border-t px-4 py-2.5 text-left first:border-t-0',
  SURFACE.hairline,
  SURFACE.hover,
  FOCUS,
  MOTION
);

/**
 * "/" once the person has anything: the newest sessions (click to resume), the desks that wait on
 * them, and ONE primary action — a new session in the folder they worked in last.
 */
function ContinuePage({
  projects,
  sessions,
  sessionsError,
  desks,
  onRetry,
}: {
  projects: DerivedProject[];
  sessions: SessionListItem[];
  sessionsError: string | null;
  desks: DeskNeed[];
  onRetry: () => void;
}) {
  const intl = useIntl();
  const navigate = useNavigate();
  const setView = useNavigation();
  const { extensionsList } = useConfig();
  const latest = projects[0];
  const projectOf = (s: SessionListItem) => folderName(s.workingDir ?? '') || s.workingDir || '';

  const newSession = async () => {
    if (!latest) return;
    try {
      await startNewSession(undefined, setView, latest.path, { allExtensions: extensionsList });
    } catch (error) {
      console.error('Failed to start a session from the landing:', error);
      toast.error(intl.formatMessage(i18n.startFailed));
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto">
      <div
        data-testid="continue-landing"
        className={cx('mx-auto flex w-full max-w-[760px] flex-col px-lz-page py-10', SPACE.section)}
      >
        <PageHeader
          title={intl.formatMessage(i18n.continueTitle)}
          subtitle={intl.formatMessage(i18n.continueSubtitle)}
          actions={
            latest ? (
              <Button variant="primary" icon={<Plus />} onClick={newSession} title={latest.path}>
                {intl.formatMessage(i18n.newSessionIn, { project: latest.name })}
              </Button>
            ) : undefined
          }
        />

        {desks.length > 0 && (
          <Panel title={intl.formatMessage(i18n.needsTitle)} count={desks.length} padded={false}>
            <ul data-testid="landing-desks">
              {desks.map((d) => (
                <li key={d.dir}>
                  <button
                    type="button"
                    className={rowButtonClass}
                    onClick={() => navigate(deskHref(d.dir))}
                    title={d.dir}
                  >
                    <span className={cx(TYPE.body, WEIGHT.medium, 'min-w-0 flex-1 truncate')}>
                      {d.name}
                    </span>
                    {d.error ? (
                      <span className="shrink-0 text-lz-meta text-lz-err">
                        {intl.formatMessage(i18n.deskUnreadable, { error: d.error })}
                      </span>
                    ) : (
                      <span className={cx(TYPE.meta, TNUM, 'shrink-0 text-lz-warn')}>
                        {[
                          d.asks > 0 && intl.formatMessage(i18n.openAsks, { count: d.asks }),
                          d.drafts > 0 &&
                            intl.formatMessage(i18n.pendingDrafts, { count: d.drafts }),
                        ]
                          .filter(Boolean)
                          .join(' · ')}
                      </span>
                    )}
                  </button>
                </li>
              ))}
            </ul>
          </Panel>
        )}

        <Panel title={intl.formatMessage(i18n.recentTitle)} count={sessions.length} padded={false}>
          {sessionsError ? (
            <div className="flex items-center gap-3 px-4 py-3">
              <span role="alert" className="min-w-0 flex-1 text-lz-body text-lz-err">
                {intl.formatMessage(i18n.sessionsFailed, { error: sessionsError })}
              </span>
              <Button size="sm" variant="secondary" onClick={onRetry}>
                {intl.formatMessage(i18n.retry)}
              </Button>
            </div>
          ) : sessions.length === 0 ? (
            <p className={cx(TYPE.bodyMuted, 'px-4 py-3')}>
              {intl.formatMessage(i18n.noSessionsYet, { project: latest?.name ?? '' })}
            </p>
          ) : (
            <ul data-testid="landing-recent-sessions">
              {sessions.map((s) => (
                <li key={s.id}>
                  <button
                    type="button"
                    data-testid={`landing-session-${s.id}`}
                    className={rowButtonClass}
                    onClick={() => navigate(`/pair?resumeSessionId=${encodeURIComponent(s.id)}`)}
                    title={s.workingDir}
                  >
                    <MessageSquare aria-hidden className="size-4 shrink-0 text-lz-ink-3" />
                    <span className={cx(TYPE.body, 'min-w-0 flex-1 truncate')}>
                      {displaySessionListName(s.name)}
                    </span>
                    <span className={cx(TYPE.meta, 'max-w-[40%] shrink-0 truncate')}>
                      {projectOf(s)}
                    </span>
                    <span className={cx(TYPE.meta, TNUM, 'w-16 shrink-0 text-right')}>
                      {timeAgo(sessionActivityAt(s))}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </Panel>
      </div>
    </div>
  );
}

/**
 * The LeanZero mark — the "L" monogram with two of the original goose flying out of it — drawn in
 * currentColor so it takes the ink of whatever accent block holds it (the sidebar brand square, the
 * landing's EmptyState block). ONE geometry for every surface: ./icons/leanzeroMark.
 */
export function LeanZeroGlyph({ className }: { className?: string }) {
  return (
    <svg
      viewBox={LEANZERO_MARK_VIEWBOX}
      fill="currentColor"
      aria-hidden
      data-testid="leanzero-glyph"
      className={className}
    >
      <LeanZeroMarkContent />
    </svg>
  );
}

/**
 * The home route ("/"). With nothing yet — no session anywhere and no folder added — it states
 * that sessions start from a project and offers the SAME add-project flow as the sidebar "+" (one
 * picker path, one broadcast). Once anything exists it is the continue page: the newest sessions,
 * the desks waiting on the person, and a new session in the folder worked in last. The projects are
 * the sidebar's (deriveProjects over the sessions and the "+" registry) — the old landing read the
 * registry alone and told a person with 29 session folders to "Add a project".
 */
export default function ProjectLanding() {
  const intl = useIntl();
  const [registry, setRegistry] = useState<ProjectEntry[] | null>(null);
  const [sessions, setSessions] = useState<SessionListItem[] | null>(null);
  const [sessionsError, setSessionsError] = useState<string | null>(null);
  const [desks, setDesks] = useState<DeskNeed[]>([]);

  const loadSessions = useCallback(async () => {
    setSessionsError(null);
    try {
      setSessions(await acpListRecentSessions(MAX_RECENT_SESSIONS));
    } catch (error) {
      console.error('Failed to load recent sessions:', error);
      setSessionsError(error instanceof Error ? error.message : String(error));
      setSessions([]);
    }
  }, []);

  useEffect(() => {
    window.electron
      .listProjects()
      .then(setRegistry)
      .catch((error) => {
        console.error('Failed to load projects:', error);
        setRegistry([]);
      });
    void loadSessions();

    const onProjectsChanged = (event: Event) => {
      const detail = (event as CustomEvent<ProjectsChangedDetail>).detail;
      if (detail) setRegistry(detail.projects);
    };
    const onDeleted = (event: Event) => {
      const { sessionId } = (event as CustomEvent<{ sessionId: string }>).detail;
      setSessions((prev) => prev?.filter((s) => s.id !== sessionId) ?? prev);
    };
    const onRenamed = (event: Event) => {
      const { sessionId, newName } = (event as CustomEvent<{ sessionId: string; newName: string }>)
        .detail;
      setSessions(
        (prev) => prev?.map((s) => (s.id === sessionId ? { ...s, name: newName } : s)) ?? prev
      );
    };
    window.addEventListener(AppEvents.PROJECTS_CHANGED, onProjectsChanged);
    window.addEventListener(AppEvents.SESSION_DELETED, onDeleted);
    window.addEventListener(AppEvents.SESSION_RENAMED, onRenamed);
    return () => {
      window.removeEventListener(AppEvents.PROJECTS_CHANGED, onProjectsChanged);
      window.removeEventListener(AppEvents.SESSION_DELETED, onDeleted);
      window.removeEventListener(AppEvents.SESSION_RENAMED, onRenamed);
    };
  }, [loadSessions]);

  useEffect(() => {
    let live = true;
    window.electron
      .agentWorkList()
      .then(readDeskNeeds)
      .then((d) => live && setDesks(d))
      .catch((error) => console.error('Failed to read the agent roster:', error));
    return () => {
      live = false;
    };
  }, []);

  const projects = useMemo(
    () => deriveProjects(sessions ?? [], registry ?? []),
    [sessions, registry]
  );
  const recent = useMemo(
    () =>
      [...(sessions ?? [])]
        .sort(
          (a, b) =>
            (Date.parse(sessionActivityAt(b)) || 0) - (Date.parse(sessionActivityAt(a)) || 0)
        )
        .slice(0, LANDING_RECENT_COUNT),
    [sessions]
  );

  const handleAddProject = async () => {
    try {
      await chooseAndAddProject();
    } catch (error) {
      console.error('Failed to add project:', error);
      toast.error(intl.formatMessage(i18n.addFailed));
    }
  };

  // Nothing is claimed before both reads land: "Start from a project" must never flash at a person
  // who has projects.
  if (registry == null || sessions == null) {
    return <div className="h-full" data-testid="landing-loading" />;
  }
  // A failed session read is not an empty install: the continue page shows the failure and Retry.
  if (projects.length > 0 || sessionsError != null) {
    return (
      <ContinuePage
        projects={projects}
        sessions={recent}
        sessionsError={sessionsError}
        desks={desks}
        onRetry={() => void loadSessions()}
      />
    );
  }

  const gives = [
    {
      key: 'dir',
      label: intl.formatMessage(i18n.giveDirLabel),
      value: intl.formatMessage(i18n.giveDirValue),
    },
    {
      key: 'sessions',
      label: intl.formatMessage(i18n.giveSessionsLabel),
      value: intl.formatMessage(i18n.giveSessionsValue),
    },
    {
      key: 'unfiled',
      label: intl.formatMessage(i18n.giveUnfiledLabel),
      value: intl.formatMessage(i18n.giveUnfiledValue),
    },
  ];

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto px-6">
      <div
        data-testid="project-landing"
        className={cx('my-auto flex w-full max-w-[560px] flex-col self-center', SPACE.section)}
      >
        <EmptyState
          icon={<LeanZeroGlyph />}
          title={intl.formatMessage(i18n.headline)}
          body={intl.formatMessage(i18n.emptyHint)}
          action={
            <Button variant="primary" icon={<FolderPlus />} onClick={() => void handleAddProject()}>
              {intl.formatMessage(i18n.addProject)}
            </Button>
          }
        />

        <Panel title={intl.formatMessage(i18n.givesTitle)} padded={false}>
          <KeyValue
            dense
            className="px-4"
            aria-label={intl.formatMessage(i18n.givesTitle)}
            items={gives}
          />
        </Panel>
      </div>
    </div>
  );
}
