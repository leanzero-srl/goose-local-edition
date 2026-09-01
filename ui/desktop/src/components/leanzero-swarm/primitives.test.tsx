import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { DownloadProgressRow } from './primitives';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

// The adapter over the Studio primitives: the download row MlxEngineView/ModelCardModal share
// (banners are studio.tsx's ToneBanner, pinned in studio.test.tsx). These pin that every state
// is a TONE — the module exports no colour string, so nothing it renders can reach the DOM as an
// inline style.
describe('leanzero primitives — download row', () => {
  it('the download row states are status tones and the bar is the accent while downloading', () => {
    const noop = () => {};
    const { getByText, getByRole, getByLabelText } = render(
      <DownloadProgressRow
        repoId="a/b"
        progress={{
          state: 'downloading',
          totalBytes: 4 * 1024 ** 3,
          downloadedBytes: 1024 ** 3,
          currentFile: 'model.safetensors',
          restartedFiles: ['x.safetensors'],
        }}
        onPause={noop}
        onResume={noop}
        onCancel={noop}
      />
    );
    expect(getByText('downloading').getAttribute('data-tone')).toBe('accent');
    expect(getByRole('progressbar').firstElementChild?.className).toContain('bg-lz-accent');
    expect(getByText('1.00 GB / 4.00 GB').className).toContain('tnum');
    expect(getByLabelText('Pause a/b')).toBeInTheDocument();
    expect(getByLabelText('Cancel a/b').title).toMatch(/DELETE/);
    expect(getByText('restarted from zero: 1 file(s)').className).toContain('text-lz-warn');
  });

  it('every class the adapter emits compiles, and nothing rendered breaks a Studio ban', async () => {
    const noop = () => {};
    const { container } = render(
      <div>
        {(['queued', 'downloading', 'paused', 'done', 'failed'] as const).map((state) => (
          <DownloadProgressRow
            key={state}
            repoId={state}
            progress={{ state, totalBytes: 10, downloadedBytes: 5, error: 'e' }}
            onPause={noop}
            onResume={noop}
            onCancel={noop}
          />
        ))}
      </div>
    );
    assertStudioClean(container);
    // lucide stamps its icon name on every <svg> (`lucide-x`); those are identities, not utilities.
    const utilities = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(utilities)).toEqual([]);
  });
});
