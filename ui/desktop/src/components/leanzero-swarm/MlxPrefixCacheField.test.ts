import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { PREFIX_CACHE_SHARE_DEFAULT, readPrefixCacheShare } from './MlxPrefixCacheField';

describe('prefix cache share', () => {
  it("the default the field names is goose-sidecar's PREFIX_CACHE_SHARE_OF_FREE", () => {
    const engine = readFileSync(
      path.join(__dirname, '../../../../../crates/goose-sidecar/src/engine.rs'),
      'utf8'
    );
    const declared = engine.match(/pub const PREFIX_CACHE_SHARE_OF_FREE: f64 = ([0-9.]+);/);
    expect(declared).not.toBeNull();
    expect(Number(declared![1])).toBe(PREFIX_CACHE_SHARE_DEFAULT);
  });

  it('reads the range the mount enforces: blank is the default, 0 < share < 1 is a share', () => {
    expect(readPrefixCacheShare('  ')).toEqual({ kind: 'default' });
    expect(readPrefixCacheShare('0.45')).toEqual({ kind: 'share', share: 0.45 });
    expect(readPrefixCacheShare(' 0.999 ')).toEqual({ kind: 'share', share: 0.999 });
    for (const bad of ['0', '1', '-0.1', '1.01', 'abc', 'Infinity', 'NaN', '0.3.1']) {
      expect(readPrefixCacheShare(bad)).toEqual({ kind: 'invalid' });
    }
  });
});
