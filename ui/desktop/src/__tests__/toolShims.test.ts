// The bundled node/npx/uvx/jbang shims sit first on goose serve's PATH, so they run inside every
// shell command the model types and every MCP server launched by name. Q-101: their setup log rode
// the command's stderr into tool results, `cd` into mcp-hermit made relative paths resolve there,
// and `tool || log` turned a failing tool into exit 0. These run the real shims against a fake
// hermit tree and hold them to the contract: the command's output, cwd, stdin and exit status are
// the command's alone, and setup is heard only in goose's log.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

const SHIM_DIR = path.resolve(__dirname, '..', 'bin');

const FAKE_HERMIT = `#!/bin/bash
echo "hermit stdout noise: $*"
echo "hermit stderr noise: $*" >&2
if [ "\${1:-}" = init ]; then
  printf 'echo "activate noise"\\n' > bin/activate-hermit
fi
if [ "\${1:-}" = install ] && [ -n "\${FAKE_HERMIT_INSTALL_FAILS:-}" ]; then
  exit 42
fi
`;

// Each fake tool reports where it ran, what it was given and what arrived on stdin, then exits
// with FAKE_TOOL_STATUS so the shim's handling of the status is observable. jbang's own setup step
// (`jbang --quiet trust add`) is setup, not the command, so it always succeeds.
const fakeTool = (name: string) => `#!/bin/bash
[ "\${2:-}" = trust ] && exit 0
printf '${name} cwd=%s args=%s stdin=%s\\n' "$PWD" "$*" "$(cat)"
exit "\${FAKE_TOOL_STATUS:-0}"
`;

let root: string;
let callerDir: string;

const writeExecutable = (file: string, body: string) => {
  fs.writeFileSync(file, body);
  fs.chmodSync(file, 0o755);
};

const runShim = (shim: string, args: string[], env: Record<string, string> = {}) =>
  spawnSync(path.join(SHIM_DIR, shim), args, {
    cwd: callerDir,
    input: 'jsonrpc-in',
    encoding: 'utf8',
    env: {
      PATH: `${SHIM_DIR}:/usr/bin:/bin`,
      HOME: root,
      GOOSE_PATH_ROOT: root,
      ...env,
    },
  });

const logText = () => fs.readFileSync(path.join(root, 'state', 'logs', 'mcp-shims.log'), 'utf8');

beforeEach(() => {
  root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'goose-shims-')));
  const hermitBin = path.join(root, 'config', 'mcp-hermit', 'bin');
  fs.mkdirSync(hermitBin, { recursive: true });
  // Without the marker the Linux one-time cleanup would delete the fake tree.
  fs.writeFileSync(path.join(root, 'config', '.mcp-hermit-cleanup-v1'), '');
  writeExecutable(path.join(hermitBin, 'hermit'), FAKE_HERMIT);
  for (const tool of ['node', 'npx', 'uvx', 'uv', 'jbang', 'java']) {
    writeExecutable(path.join(hermitBin, tool), fakeTool(tool));
  }
  callerDir = path.join(root, 'project');
  fs.mkdirSync(callerDir);
});

afterEach(() => {
  fs.rmSync(root, { recursive: true, force: true });
});

describe.skipIf(process.platform === 'win32')('bundled tool shims', () => {
  it.each([
    ['node', 'node'],
    ['npx', 'npx'],
    ['uvx', 'uvx'],
    ['jbang', 'jbang'],
  ])('%s: output, cwd and stdin are the command’s; setup goes to the log', (shim, tool) => {
    const result = runShim(shim, ['scripts/run.js', '--flag']);

    const expectedArgs =
      shim === 'jbang' ? '--fresh --quiet scripts/run.js --flag' : 'scripts/run.js --flag';
    expect(result.stderr).toBe('');
    expect(result.stdout).toBe(`${tool} cwd=${callerDir} args=${expectedArgs} stdin=jsonrpc-in\n`);
    expect(result.status).toBe(0);

    const log = logText();
    expect(log).toContain(`[${shim} `);
    expect(log).toContain('hermit stdout noise: init');
    expect(log).toContain('hermit stderr noise: init');
    expect(log).toContain('hermit stdout noise: install');
    expect(log).toContain(`Executing ${path.join(root, 'config', 'mcp-hermit', 'bin', tool)}`);
  });

  it.each(['node', 'npx', 'uvx', 'jbang'])(
    '%s: a failing command keeps its own exit status',
    (shim) => {
      const result = runShim(shim, ['missing.js'], { FAKE_TOOL_STATUS: '3' });

      expect(result.status).toBe(3);
      expect(result.stderr).toBe('');
      expect(logText()).not.toContain('Setup failed');
    }
  );

  it('a second run with hermit already initialised is just as quiet', () => {
    runShim('node', ['--version']);
    const result = runShim('node', ['--version']);

    expect(result.stderr).toBe('');
    expect(result.stdout).toBe(`node cwd=${callerDir} args=--version stdin=jsonrpc-in\n`);
  });

  it.each(['node', 'uvx', 'jbang'])(
    '%s: a failed setup says so in one stderr line naming the log, and never runs the tool',
    (shim) => {
      const result = runShim(shim, ['app.js'], { FAKE_HERMIT_INSTALL_FAILS: '1' });

      const logFile = path.join(root, 'state', 'logs', 'mcp-shims.log');
      expect(result.status).toBe(42);
      expect(result.stdout).toBe('');
      expect(result.stderr).toBe(`goose: ${shim} setup failed (status 42); see ${logFile}\n`);
      expect(logText()).toContain('Setup failed with status 42.');
      expect(fs.existsSync(path.join(root, 'config', '.mcp-hermit-setup.lock'))).toBe(false);
    }
  );

  it('appends to the log instead of truncating it per invocation', () => {
    runShim('node', ['first.js']);
    runShim('uvx', ['second']);

    const log = logText();
    expect(log).toContain('first.js');
    expect(log).toContain('second');
  });
});
