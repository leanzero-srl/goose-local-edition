import type React from 'react';
import { describe, expect, it, vi } from 'vitest';
import { render as rtlRender, screen, waitFor } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { ModelCardModal } from './ModelCardModal';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

const mockModelCard = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineModelCard: (...args: unknown[]) => mockModelCard(...args),
}));

const GB = 1024 ** 3;
const CARD = {
  readmeMarkdown: '# Readme heading\n\nBody.',
  readmeTruncated: true,
  files: [
    { path: 'config.json', sizeBytes: 1200 },
    { path: 'model-00001-of-00002.safetensors', sizeBytes: 5 * GB },
  ],
  totalBytes: 5 * GB + 1200,
  tags: ['mlx', '4-bit'],
  downloads: 12800,
  likes: 42,
  license: 'apache-2.0',
  createdAt: '2026-08-20T10:00:00Z',
  lastModified: '2026-08-25T10:00:00Z',
};

const noop = () => {};

// MarkdownContent (the README renderer) needs the app's intl context.
const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });

// The card on the Studio primitives: a floating header Panel with KeyValue facts, the files
// as a DataTable, and nothing on the tree that breaks a ban or names a class that does not compile.
describe('ModelCardModal — Studio register', () => {
  it('facts are KeyValue rows, files are a table with the exact total, every class compiles', async () => {
    mockModelCard.mockResolvedValue(CARD);
    const { unmount } = render(
      <ModelCardModal
        repoId="mlx-community/Some-Model-4bit"
        onClose={noop}
        progress={{ state: 'downloading', totalBytes: 4 * GB, downloadedBytes: GB }}
        startError={undefined}
        onDownload={noop}
        onPause={noop}
        onResume={noop}
        onCancel={noop}
      />
    );
    await waitFor(() => {
      expect(screen.getByText('apache-2.0')).toBeInTheDocument();
    });
    expect(mockModelCard).toHaveBeenCalledWith('mlx-community/Some-Model-4bit', undefined);
    const facts = screen.getByLabelText('Model facts');
    expect(facts).toHaveTextContent('Downloads');
    expect(facts).toHaveTextContent('12.8K');
    expect(facts).toHaveTextContent('Size (exact)');
    const files = screen.getByRole('table', { name: 'Repository files' });
    expect(files).toHaveTextContent('model-00001-of-00002.safetensors');
    expect(screen.getByText('5.00 GB total')).toBeInTheDocument();
    expect(screen.getByTestId('lz-section-count')).toHaveTextContent('2');
    // The publisher and the tags are quiet chips; the ONE accent is the truncation link
    // (Download is hidden while the download is live).
    expect(screen.getByText('mlx-community').getAttribute('data-tone')).toBeNull();
    expect(screen.getByText('mlx').getAttribute('data-tone')).toBeNull();
    expect(screen.getByText(/read the full page on huggingface\.co/).className).toContain(
      'text-lz-accent'
    );
    expect(screen.getByTestId('mlx-download-mlx-community/Some-Model-4bit')).toBeInTheDocument();
    assertStudioClean(document.body);
    // The README rides through the app's MarkdownContent (its own prose-* classes are the
    // renderer's, not the Studio's); every class the card itself emits must compile.
    const own = document.body.cloneNode(true) as HTMLElement;
    own.querySelector('[data-testid="mlx-model-card-readme"]')?.remove();
    const utilities = allClasses(own).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(utilities)).toEqual([]);
    unmount();
  });

  it('a failed card is a loud banner; an absent README is honest; Download is the primary when idle', async () => {
    mockModelCard.mockRejectedValueOnce(new Error('HF 503'));
    const first = render(
      <ModelCardModal
        repoId="a/b"
        onClose={noop}
        progress={undefined}
        startError="start refused"
        onDownload={noop}
        onPause={noop}
        onResume={noop}
        onCancel={noop}
      />
    );
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('HF 503');
    });
    expect(screen.getByText('start refused').className).toContain('text-lz-err');
    expect(screen.getByLabelText('Download a/b').className).toContain('bg-lz-accent');
    assertStudioClean(document.body);
    first.unmount();

    mockModelCard.mockResolvedValueOnce({
      ...CARD,
      readmeMarkdown: undefined,
      readmeTruncated: false,
      tags: [],
    });
    const second = render(
      <ModelCardModal
        repoId="a/b"
        onClose={noop}
        progress={undefined}
        startError={undefined}
        onDownload={noop}
        onPause={noop}
        onResume={noop}
        onCancel={noop}
      />
    );
    await waitFor(() => {
      expect(screen.getByText('This repo has no README.')).toBeInTheDocument();
    });
    second.unmount();
  });
});
