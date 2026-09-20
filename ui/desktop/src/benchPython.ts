/** Signed harness imports must not create or rewrite bytecode inside app Resources. */
export function benchmarkPythonLaunch(runner: string, args: string[], env: NodeJS.ProcessEnv) {
  return { args: ['-B', '-u', runner, ...args], env: { ...env, PYTHONDONTWRITEBYTECODE: '1' } };
}
