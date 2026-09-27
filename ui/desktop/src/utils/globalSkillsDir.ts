/**
 * Where goose's global skills live, as main resolved it (utils/goosePaths `gooseGlobalSkillsDir`,
 * the mirror of `Paths::agents_home_dir()/skills`): `~/.agents/skills`, or `<GOOSE_PATH_ROOT>/.agents/skills`
 * for an isolated profile (Q-188 — the renderer wrote `~/.agents/skills` literally, so an isolated
 * profile's skill import landed in the owner's home). Main puts it in every window's appConfig; a
 * window without it is a wiring defect, and guessing the owner's folder is exactly the leak.
 */
export const getGlobalSkillsDir = (): string => {
  const dir = window.appConfig?.get('GOOSE_GLOBAL_SKILLS_DIR');
  if (typeof dir !== 'string' || dir.length === 0) {
    throw new Error(
      'GOOSE_GLOBAL_SKILLS_DIR is missing from appConfig (main sets it for every window)'
    );
  }
  return dir;
};
