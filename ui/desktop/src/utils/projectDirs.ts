import fs from 'fs';
import path from 'path';
import { app } from 'electron';

/**
 * User-curated projects registry (projects.json in userData) — the Projects tree's source of truth.
 * Unlike recent-dirs.json this is NOT an LRU: entries are added and removed only by explicit user
 * action, never aged out. Removing an entry edits this file and nothing else — no directory on
 * disk and no session is ever touched.
 */
export interface ProjectEntry {
  path: string;
  addedAt: number;
}

interface ProjectsFile {
  projects: ProjectEntry[];
}

function projectsFilePath(): string {
  return path.join(app.getPath('userData'), 'projects.json');
}

/** Same validation stance as recentDirs: must exist, must be a directory, symlinks rejected. */
function isUsableDir(dir: string): boolean {
  try {
    const stats = fs.lstatSync(dir);
    if (stats.isSymbolicLink()) {
      console.warn('Rejecting project directory: symlinks not allowed for security');
      return false;
    }
    return stats.isDirectory();
  } catch {
    return false;
  }
}

function readProjectsFile(): ProjectEntry[] {
  try {
    const file = projectsFilePath();
    if (!fs.existsSync(file)) {
      return [];
    }
    const parsed = JSON.parse(fs.readFileSync(file, 'utf8')) as ProjectsFile;
    if (!Array.isArray(parsed?.projects)) {
      return [];
    }
    return parsed.projects.filter(
      (p): p is ProjectEntry => !!p && typeof p.path === 'string' && typeof p.addedAt === 'number'
    );
  } catch (error) {
    console.error('Error loading projects registry:', error);
    return [];
  }
}

function writeProjectsFile(projects: ProjectEntry[]): void {
  fs.writeFileSync(projectsFilePath(), JSON.stringify({ projects }, null, 2));
}

export function loadProjects(): ProjectEntry[] {
  const projects = readProjectsFile();
  const valid = projects.filter((p) => isUsableDir(p.path));
  if (valid.length !== projects.length) {
    try {
      writeProjectsFile(valid);
    } catch (error) {
      console.error('Error pruning projects registry:', error);
    }
  }
  return valid;
}

export function addProject(dir: string): ProjectEntry[] {
  const existing = loadProjects();
  if (!dir || !path.isAbsolute(dir)) {
    return existing;
  }
  const canonical = path.resolve(dir);
  if (!isUsableDir(canonical)) {
    return existing;
  }
  if (existing.some((p) => p.path === canonical)) {
    return existing;
  }
  const next = [{ path: canonical, addedAt: Date.now() }, ...existing];
  try {
    writeProjectsFile(next);
  } catch (error) {
    console.error('Error saving projects registry:', error);
    return existing;
  }
  return next;
}

/** Removes the entry from the registry file ONLY — the directory and its sessions are untouched. */
export function removeProject(dir: string): ProjectEntry[] {
  if (!dir || !path.isAbsolute(dir)) {
    return loadProjects();
  }
  const canonical = path.resolve(dir);
  const existing = readProjectsFile();
  const next = existing.filter((p) => p.path !== canonical);
  if (next.length !== existing.length) {
    try {
      writeProjectsFile(next);
    } catch (error) {
      console.error('Error saving projects registry:', error);
    }
  }
  return next.filter((p) => isUsableDir(p.path));
}
