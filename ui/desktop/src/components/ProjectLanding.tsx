import { useEffect, useState } from 'react';
import { FolderPlus } from 'lucide-react';
import { toast } from 'react-toastify';
import { defineMessages, useIntl } from '../i18n';
import { AppEvents } from '../constants/events';
import { chooseAndAddProject, type ProjectsChangedDetail } from '../utils/addProjectFlow';
import type { ProjectEntry } from '../utils/projectDirs';
import { Button, EmptyState, KeyValue, Panel, SPACE, cx } from './lz';
import { LEANZERO_MARK_VIEWBOX, LeanZeroMarkContent } from './icons/leanzeroMark';

const i18n = defineMessages({
  headline: {
    id: 'projectLanding.headline',
    defaultMessage: 'Start from a project',
  },
  pickHint: {
    id: 'projectLanding.pickHint',
    defaultMessage: 'Pick a project in the sidebar and use its “+ New session here”.',
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
    defaultMessage: 'Outside a project',
  },
  giveUnfiledValue: {
    id: 'projectLanding.giveUnfiledValue',
    defaultMessage: 'Kept in Unfiled',
  },
});

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
 * The home route ("/") — pass D (owner): no chat input lives here anymore. Sessions start from a
 * project, so with no active session the main pane states that fact and, when no project exists
 * yet, offers the SAME add-project flow as the sidebar "+" (one picker path, one broadcast).
 * Opening an existing session (project row or Unfiled) still goes through /pair, untouched.
 */
export default function ProjectLanding() {
  const intl = useIntl();
  const [projects, setProjects] = useState<ProjectEntry[] | null>(null);

  useEffect(() => {
    window.electron
      .listProjects()
      .then(setProjects)
      .catch((error) => {
        console.error('Failed to load projects:', error);
        setProjects([]);
      });

    const onProjectsChanged = (event: Event) => {
      const detail = (event as CustomEvent<ProjectsChangedDetail>).detail;
      if (detail) setProjects(detail.projects);
    };
    window.addEventListener(AppEvents.PROJECTS_CHANGED, onProjectsChanged);
    return () => window.removeEventListener(AppEvents.PROJECTS_CHANGED, onProjectsChanged);
  }, []);

  const handleAddProject = async () => {
    try {
      await chooseAndAddProject();
    } catch (error) {
      console.error('Failed to add project:', error);
      toast.error(intl.formatMessage(i18n.addFailed));
    }
  };

  const hasProjects = projects != null && projects.length > 0;

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
          body={
            projects == null
              ? undefined
              : intl.formatMessage(hasProjects ? i18n.pickHint : i18n.emptyHint)
          }
          action={
            projects != null && !hasProjects ? (
              <Button
                variant="primary"
                icon={<FolderPlus />}
                onClick={() => void handleAddProject()}
              >
                {intl.formatMessage(i18n.addProject)}
              </Button>
            ) : undefined
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
