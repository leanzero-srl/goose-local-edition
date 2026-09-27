/**
 * goose's config dir as main resolved it (utils/goosePaths `gooseDirs().config`, the mirror of
 * `Paths::config_dir()`), in the form a person reads and a model is told: `~/.config/goose`, or
 * `<GOOSE_PATH_ROOT>/config` for an isolated profile. Q-198: the MCP prompt named
 * `~/.config/goose/config.yaml` literally, so under a root the model read and edited the owner's
 * config. Main puts it in every window's appConfig; a window without it is a wiring defect, and
 * guessing the owner's folder is exactly the leak (as utils/globalSkillsDir, Q-188).
 */
export const getGooseConfigDir = (): string => {
  const dir = window.appConfig?.get('GOOSE_CONFIG_DIR');
  if (typeof dir !== 'string' || dir.length === 0) {
    throw new Error('GOOSE_CONFIG_DIR is missing from appConfig (main sets it for every window)');
  }
  return dir;
};
