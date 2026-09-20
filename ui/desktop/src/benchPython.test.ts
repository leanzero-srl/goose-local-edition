import { expect, it } from 'vitest';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { benchmarkPythonLaunch } from './benchPython';
it('imports harness modules and child Python without mutating signed source directories', async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'signed-harness-'));
  try {
    const module = 'VALUE = 42\n';
    await fs.writeFile(path.join(dir, 'private_module.py'), module);
    const runner = path.join(dir, 'runner.py');
    await fs.writeFile(
      runner,
      "import private_module, subprocess, sys\nassert private_module.VALUE == 42\nsubprocess.run([sys.executable, '-c', 'import private_module'], check=True)\n"
    );
    const launch = benchmarkPythonLaunch(runner, [], {
      ...process.env,
      PYTHONDONTWRITEBYTECODE: '0',
    });
    execFileSync('python3', launch.args, { cwd: dir, env: launch.env });
    expect((await fs.readdir(dir)).sort()).toEqual(['private_module.py', 'runner.py']);
    expect(await fs.readFile(path.join(dir, 'private_module.py'), 'utf8')).toBe(module);
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
});
