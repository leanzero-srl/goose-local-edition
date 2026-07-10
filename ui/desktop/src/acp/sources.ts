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
