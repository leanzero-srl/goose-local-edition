import path from 'node:path';

/** Explicit Goose profiles must not borrow another installation's results or public identity. */
export function benchmarkProfileDirectory(home: string, goosePathRoot?: string): string {
  return goosePathRoot ? path.join(goosePathRoot, 'config', 'benchmark') : path.join(home, '.config', 'goose', 'benchmark');
}
