import path from 'node:path';
import { gooseDirsFor } from './utils/goosePaths';

/** Explicit Goose profiles must not borrow another installation's results or public identity. */
export function benchmarkProfileDirectory(home: string, goosePathRoot?: string): string {
  return path.join(gooseDirsFor(home, goosePathRoot).config, 'benchmark');
}
