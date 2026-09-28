import fs from 'node:fs';
import path from 'node:path';
import ts from 'typescript';
import { expect, it } from 'vitest';

/**
 * Q-383 — no wait in a test keeps a clock of its own. setup.ts gives testing-library's waits the
 * running test's timeout (testClock.ts); a `{ timeout: N }` on a findBy / waitFor puts a shorter
 * clock back, and vitest's own vi.waitFor / vi.waitUntil default to 1 s unless handed testClock().
 * Every such wait passed alone and failed in full runs beside a cargo build.
 */
const SRC = path.resolve(__dirname, '..');
const TEST_FILE = /\.(test|spec)\.[cm]?[jt]sx?$/;
const TESTING_LIBRARY_WAIT = /^(waitFor|waitForElementToBeRemoved|find(All)?By[A-Z]\w*)$/;
const VITEST_WAIT = new Set(['waitFor', 'waitUntil']);

const hasTimeout = (node: ts.Node): node is ts.ObjectLiteralExpression =>
  ts.isObjectLiteralExpression(node) &&
  node.properties.some((p) => p.name !== undefined && ts.isIdentifier(p.name) && p.name.text === 'timeout');

const runsOnTestClock = (options: ts.Expression | undefined, sf: ts.SourceFile): boolean =>
  options !== undefined &&
  ts.isObjectLiteralExpression(options) &&
  options.properties.some(
    (p) =>
      ts.isPropertyAssignment(p) &&
      ts.isIdentifier(p.name) &&
      p.name.text === 'timeout' &&
      p.initializer.getText(sf) === 'testClock()'
  );

function waitsOnTheirOwnClock(file: string): string[] {
  const text = fs.readFileSync(file, 'utf8');
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  const found: string[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isCallExpression(node)) {
      const callee = node.expression;
      const name = ts.isIdentifier(callee)
        ? callee.text
        : ts.isPropertyAccessExpression(callee)
          ? callee.name.text
          : '';
      const owner = ts.isPropertyAccessExpression(callee) ? callee.expression.getText(sf) : '';
      const offends =
        owner === 'vi'
          ? VITEST_WAIT.has(name) && !runsOnTestClock(node.arguments[1], sf)
          : TESTING_LIBRARY_WAIT.test(name) && node.arguments.some(hasTimeout);
      if (offends) {
        const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
        found.push(`${path.relative(SRC, file)}:${line + 1} ${node.getText(sf).split('\n')[0]}`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return found;
}

it('every wait in a test runs on the test’s own clock (Q-383)', () => {
  const files = (fs.readdirSync(SRC, { recursive: true }) as string[])
    .filter((f) => TEST_FILE.test(f))
    .map((f) => path.join(SRC, f));
  expect(files.length).toBeGreaterThan(100);
  expect(files.flatMap(waitsOnTheirOwnClock)).toEqual([]);
});
