import { AppEvents } from '../constants/events';
import { getInitialWorkingDir } from './workingDir';
import type { ProjectEntry } from './projectDirs';

export interface ProjectsChangedDetail {
  projects: ProjectEntry[];
  added: ProjectEntry[];
}

/**
 * THE add-project flow — the one picker path shared by every surface that registers a project
 * (the sidebar's Projects "+" and the home landing's Add-project button). Runs the OS directory
 * chooser seeded with the window's working dir, registers the pick, and broadcasts
 * PROJECTS_CHANGED so every mounted surface (ProjectsSection expands the new project, the landing
 * flips its empty state) follows the same fact.
 *
 * Returns null when the user cancels the picker.
 */
export async function chooseAndAddProject(): Promise<ProjectsChangedDetail | null> {
  const seed = getInitialWorkingDir();
  const result = await window.electron.directoryChooser(seed || undefined);
  if (result.canceled || result.filePaths.length === 0) return null;

  const before = await window.electron.listProjects();
  const previousPaths = new Set(before.map((p) => p.path));
  const projects = await window.electron.addProject(result.filePaths[0]);
  const added = projects.filter((p) => !previousPaths.has(p.path));

  const detail: ProjectsChangedDetail = { projects, added };
  window.dispatchEvent(new CustomEvent(AppEvents.PROJECTS_CHANGED, { detail }));
  return detail;
}

/** Removal goes through the registry only, then broadcasts the same change event. */
export async function removeProjectAndBroadcast(projectPath: string): Promise<ProjectEntry[]> {
  const projects = await window.electron.removeProject(projectPath);
  window.dispatchEvent(
    new CustomEvent(AppEvents.PROJECTS_CHANGED, {
      detail: { projects, added: [] } satisfies ProjectsChangedDetail,
    })
  );
  return projects;
}
