import type { SourceEntry, SourceType, SourceScope } from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

const SKILL_SOURCE_TYPES: SourceType[] = ['skill', 'builtinSkill'];

/**
 * Create a skill source from parsed SKILL.md fields. `content` MUST be the markdown BODY ONLY (frontmatter
 * stripped) — the backend regenerates the frontmatter from name/description. Errors if a skill with that
 * name already exists (create does not auto-suffix). Used by the Claude Code import tool for body-only
 * skills; skills with supporting files are relocated by copying the directory instead.
 */
export async function createSkillSource(params: {
  name: string;
  description: string;
  content: string;
  target: SourceScope;
}): Promise<SourceEntry> {
  const client = await getAcpClient();
  const { source } = await client.goose.sourcesCreate_unstable({
    type: 'skill',
    name: params.name,
    description: params.description,
    content: params.content,
    target: params.target,
  });
  return source;
}
/**
 * Write a skill's body back to disk. `content` is the markdown BODY — the backend regenerates the frontmatter
 * from name/description, exactly as create does.
 *
 * `properties` is deliberately omitted: the request's own contract says an omitted bag preserves the source's
 * existing properties, while sending one REPLACES all of them. This editor models name/description/content
 * only, so passing anything here would silently erase per-skill metadata it never showed the user.
 *
 * Only `type: 'skill'` may be updated. Built-ins are rejected by `require_mutable_type` on the backend, and
 * `entry.writable` cannot be used to predict that — it is hardcoded true for every skill including built-ins.
 */
export async function updateSkillSource(params: {
  path: string;
  name: string;
  description: string;
  content: string;
}): Promise<SourceEntry> {
  const client = await getAcpClient();
  const { source } = await client.goose.sourcesUpdate_unstable({
    type: 'skill',
    path: params.path,
    name: params.name,
    description: params.description,
    content: params.content,
  });
  return source;
}

/** Delete a skill and its on-disk directory. Irreversible — there is no trash. Confirm before calling. */
export async function deleteSkillSource(path: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.sourcesDelete_unstable({ type: 'skill', path });
}

/**
 * Read ONE skill's current on-disk state, bypassing the dedup cache below.
 *
 * This exists for the save path's conflict check and MUST NOT be routed through `listSkillSources`: that
 * function returns any in-flight promise for the same projectDir, so a check issued while another caller
 * already has a load running would compare against data fetched BEFORE the write it is trying to guard —
 * passing the check precisely when a conflict exists. The swarm engine rewrites a persona's SKILL.md from a
 * separate process with a truncating write and no locking, so this is a real race, not a theoretical one.
 */
export async function readSkillSourceFresh(
  projectDir: string,
  path: string
): Promise<SourceEntry | undefined> {
  const client = await getAcpClient();
  const { sources } = await client.goose.sourcesList_unstable({ type: 'skill', projectDir });
  return sources.find((s) => s.path === path);
}

const inFlightSkillSourceLoads = new Map<string, Promise<SourceEntry[]>>();

export async function listSkillSources(projectDir: string): Promise<SourceEntry[]> {
  const inFlightLoad = inFlightSkillSourceLoads.get(projectDir);
  if (inFlightLoad) {
    return inFlightLoad;
  }

  const load = loadSkillSources(projectDir);
  inFlightSkillSourceLoads.set(projectDir, load);

  try {
    return await load;
  } finally {
    if (inFlightSkillSourceLoads.get(projectDir) === load) {
      inFlightSkillSourceLoads.delete(projectDir);
    }
  }
}

async function loadSkillSources(projectDir: string): Promise<SourceEntry[]> {
  const client = await getAcpClient();
  const responses = await Promise.all(
    SKILL_SOURCE_TYPES.map((type) =>
      client.goose.sourcesList_unstable({
        type,
        projectDir,
      })
    )
  );

  return responses
    .flatMap((response) => response.sources)
    .sort(
      (a, b) =>
        a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
        a.path.localeCompare(b.path)
    );
}
