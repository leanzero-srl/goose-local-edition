import { useEffect, useState } from 'react';
import { FolderPlus, PanelLeft } from 'lucide-react';
import { toast } from 'react-toastify';
import { defineMessages, useIntl } from '../i18n';
import { AppEvents } from '../constants/events';
import { chooseAndAddProject, type ProjectsChangedDetail } from '../utils/addProjectFlow';
import type { ProjectEntry } from '../utils/projectDirs';

const i18n = defineMessages({
  headline: {
    id: 'projectLanding.headline',
    defaultMessage: 'Sessions start from a project',
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
});

// Benchmark register: solid saturated fills, full borders — mirrors the Projects tree's
// first hue so the landing and the sidebar "+" read as the same affordance.
const AZURE = '#2e8bff';

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

  return (
    <div className="flex h-full min-h-0 flex-col items-center justify-center px-6">
      <div
        data-testid="project-landing"
        className="w-full max-w-xl overflow-hidden rounded border border-border-primary"
      >
        <div className="flex items-center gap-2 border-b border-border-primary bg-background-secondary px-4 py-2">
          <span
            className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded"
            style={{ backgroundColor: AZURE }}
          >
            <FolderPlus className="h-3.5 w-3.5" strokeWidth={2.5} style={{ color: '#0b0b0b' }} />
          </span>
          <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
            Projects
          </span>
        </div>

        <div className="flex flex-col gap-3 px-4 py-5">
          <h1 className="text-2xl font-bold text-text-primary">
            {intl.formatMessage(i18n.headline)}
          </h1>

          {projects == null ? null : hasProjects ? (
            <p className="flex items-center gap-2 text-sm text-text-secondary">
              <PanelLeft className="h-4 w-4 shrink-0" style={{ color: AZURE }} />
              {intl.formatMessage(i18n.pickHint)}
            </p>
          ) : (
            <>
              <p className="text-sm text-text-secondary">{intl.formatMessage(i18n.emptyHint)}</p>
              <button
                type="button"
                onClick={() => void handleAddProject()}
                className="inline-flex items-center gap-2 self-start rounded px-4 py-2 text-sm font-bold text-white transition-opacity hover:opacity-90"
                style={{ backgroundColor: AZURE }}
              >
                <FolderPlus className="h-4 w-4" strokeWidth={2.5} />
                {intl.formatMessage(i18n.addProject)}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
