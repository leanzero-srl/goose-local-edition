import os from 'node:os';
import path from 'node:path';
import { expandTilde } from './pathUtils';

/**
 * The main process's ONE resolver for goose's own directories — the mirror of
 * crates/goose/src/config/paths.rs. With GOOSE_PATH_ROOT set, every dir hangs from the root
 * (config → <root>/config, data → <root>/data, state → <root>/state), so an isolated profile
 * (the harness's packaged build, a second install) never reads or writes the owner's files
 * (Q-183: memories, proposals and the agent-work registry were read from ~/.config/goose
 * while goosed wrote under the root). Unset, the paths are the XDG layout the desktop has
 * always used (~/.config/goose, ~/.local/share/goose, ~/.local/state/goose).
 *
 * Resolved per call from the environment: main never sets GOOSE_PATH_ROOT itself, it only
 * inherits it, so a lazily-read root and one read at import are the same value — but a
 * module-level path const would not follow a test's stubbed env.
 */
export interface GooseDirs {
  config: string;
  data: string;
  state: string;
}

export function resolveGoosePathRoot(): string | undefined {
  const pathRoot = process.env.GOOSE_PATH_ROOT?.trim();
  return pathRoot ? expandTilde(pathRoot) : undefined;
}

/** PURE: the dirs for an explicit home and root (undefined root = the owner's home layout). */
export function gooseDirsFor(home: string, pathRoot: string | undefined): GooseDirs {
  if (pathRoot) {
    return {
      config: path.join(pathRoot, 'config'),
      data: path.join(pathRoot, 'data'),
      state: path.join(pathRoot, 'state'),
    };
  }
  return {
    config: path.join(home, '.config', 'goose'),
    data: path.join(home, '.local', 'share', 'goose'),
    state: path.join(home, '.local', 'state', 'goose'),
  };
}

export function gooseDirs(): GooseDirs {
  return gooseDirsFor(os.homedir(), resolveGoosePathRoot());
}

export const gooseConfigYamlPath = (): string => path.join(gooseDirs().config, 'config.yaml');

/** Global memories (goose-mcp memory: `Paths::config_dir().join("memory")`). */
export const gooseGlobalMemoryDir = (): string => path.join(gooseDirs().config, 'memory');

/** Saved memory proposals — the sibling of the global memory dir (goose-mcp memory `proposals_dir`). */
export const gooseMemoryProposalsDir = (): string => path.join(gooseDirs().config, 'proposals');

/**
 * PURE: goose's `.agents` home — the mirror of `Paths::agents_home_dir()`: `<root>/.agents` under a
 * root, `~/.agents` unset.
 */
export function gooseAgentsHomeFor(home: string, pathRoot: string | undefined): string {
  return path.join(pathRoot ?? home, '.agents');
}

/**
 * Global skills — `crates/goose/src/skills` `global_skills_dir()` (Q-188): where goose reads them and
 * where the Claude Code import copies them. Unset root: `~/.agents/skills`, as always.
 */
export const gooseGlobalSkillsDir = (): string =>
  path.join(gooseAgentsHomeFor(os.homedir(), resolveGoosePathRoot()), 'skills');

/**
 * PURE: a path as the renderer shows it and hands it back over IPC — `~/…` under the home folder
 * (main's file handlers expand the tilde), the full path outside it.
 */
export function homeRelative(p: string, home: string): string {
  const rest = path.relative(home, p);
  if (rest === '' || rest.startsWith('..') || path.isAbsolute(rest)) return p;
  return `~/${rest.split(path.sep).join('/')}`;
}

/** The desktop's registry of agent-work directories. */
export const agentWorkRegistryPath = (): string => path.join(gooseDirs().config, 'agent-work.json');
