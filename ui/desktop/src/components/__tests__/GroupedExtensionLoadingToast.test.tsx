import { describe, it, expect } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { GroupedExtensionLoadingToast } from '../GroupedExtensionLoadingToast';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

const renderWithRouter = (component: React.ReactElement) => {
  return render(
    <IntlTestWrapper>
      <MemoryRouter>{component}</MemoryRouter>
    </IntlTestWrapper>
  );
};

describe('GroupedExtensionLoadingToast', () => {
  it('renders loading state correctly', () => {
    const extensions = [
      { name: 'developer', status: 'loading' as const },
      { name: 'memory', status: 'loading' as const },
    ];

    renderWithRouter(
      <GroupedExtensionLoadingToast extensions={extensions} totalCount={2} isComplete={false} />
    );

    expect(screen.getByText('Loading 2 extensions...')).toBeInTheDocument();
    expect(screen.getByText('Show details')).toBeInTheDocument();
  });

  it('renders success state correctly', () => {
    const extensions = [
      { name: 'developer', status: 'success' as const },
      { name: 'memory', status: 'success' as const },
    ];

    renderWithRouter(
      <GroupedExtensionLoadingToast extensions={extensions} totalCount={2} isComplete={true} />
    );

    expect(screen.getByText('Successfully loaded 2 extensions')).toBeInTheDocument();
    expect(screen.getByText('Show details')).toBeInTheDocument();
  });

  it('renders partial failure state correctly', () => {
    const extensions = [
      { name: 'developer', status: 'success' as const },
      { name: 'memory', status: 'error' as const, error: 'Failed to connect' },
    ];

    renderWithRouter(
      <GroupedExtensionLoadingToast extensions={extensions} totalCount={2} isComplete={true} />
    );

    expect(screen.getByText('Loaded 1/2 extensions')).toBeInTheDocument();
    expect(screen.getByText('1 extension failed to load')).toBeInTheDocument();
    expect(screen.getByText('Show details')).toBeInTheDocument();
  });

  it('renders single extension correctly', () => {
    const extensions = [{ name: 'developer', status: 'success' as const }];

    renderWithRouter(
      <GroupedExtensionLoadingToast extensions={extensions} totalCount={1} isComplete={true} />
    );

    expect(screen.getByText('Successfully loaded 1 extension')).toBeInTheDocument();
  });

  it('renders mixed status states correctly', () => {
    const extensions = [
      { name: 'developer', status: 'success' as const },
      { name: 'memory', status: 'loading' as const },
      { name: 'Square MCP Server', status: 'error' as const, error: 'Connection failed' },
    ];

    renderWithRouter(
      <GroupedExtensionLoadingToast extensions={extensions} totalCount={3} isComplete={false} />
    );

    // Summary should show loading state with error count
    expect(screen.getByText('Loading 3 extensions...')).toBeInTheDocument();
    expect(screen.getByText('1 extension failed to load')).toBeInTheDocument();
    expect(screen.getByText('Show details')).toBeInTheDocument();
  });
});

/**
 * Studio remake: each state is a StatusDot (accent+live / ok / warn / err), the summary is the
 * body step in the semibold weight, the details are a quiet meta disclosure, "Copy error" is a
 * secondary lz Button. No black blob, no green blob, no opacity.
 */
describe('GroupedExtensionLoadingToast (Studio)', () => {
  it('a clean load is an ok dot and the disclosure is in the meta register', async () => {
    const { container } = renderWithRouter(
      <GroupedExtensionLoadingToast
        extensions={[{ name: 'developer', status: 'success' }]}
        totalCount={1}
        isComplete
      />
    );
    const dot = screen.getByRole('img', { name: 'Loaded' });
    expect(dot.className).toContain('bg-lz-ok');
    expect(dot.className).toContain('size-2.5');
    const title = screen.getByText('Successfully loaded 1 extension');
    expect(title.className).toContain('text-lz-body');
    expect(title.className).toContain('font-lz-semibold');
    const toggle = screen.getByTestId('extension-loading-toggle');
    expect(toggle.className).toContain('text-lz-meta');
    expect(toggle.className).toContain('text-lz-ink-3');
    expect(allClasses(container)).not.toEqual(
      expect.arrayContaining(['bg-green-500', 'bg-black', 'text-white'])
    );
    assertStudioClean(container);
    expect(
      await missingUtilities(allClasses(container).filter((c) => !c.startsWith('lucide')))
    ).toEqual([]);
  }, 30_000);

  it('while loading the summary dot is the live accent', () => {
    renderWithRouter(
      <GroupedExtensionLoadingToast
        extensions={[{ name: 'developer', status: 'loading' }]}
        totalCount={1}
        isComplete={false}
      />
    );
    const dot = screen.getByRole('img', { name: 'Loading' });
    expect(dot.className).toContain('bg-lz-accent');
    expect(dot.getAttribute('data-live')).toBe('true');
  });

  it('a partial load is a warn summary; the opened details carry an err dot and a secondary Copy error', async () => {
    const { container } = renderWithRouter(
      <GroupedExtensionLoadingToast
        extensions={[
          { name: 'developer', status: 'success' },
          { name: 'memory', status: 'error', error: 'Failed to connect' },
        ]}
        totalCount={2}
        isComplete
      />
    );
    expect(screen.getByRole('img', { name: 'Partially loaded' }).className).toContain('bg-lz-warn');
    expect(screen.getByText('1 extension failed to load').className).toContain('text-lz-err');
    fireEvent.click(screen.getByTestId('extension-loading-toggle'));
    await waitFor(() => expect(screen.getByText('Show less')).toBeInTheDocument());
    expect(screen.getByRole('img', { name: 'Failed' }).className).toContain('bg-lz-err');
    expect(screen.getByRole('img', { name: 'Loaded' }).className).toContain('bg-lz-ok');
    const copy = screen.getByRole('button', { name: 'Copy error' });
    expect(copy.getAttribute('data-variant')).toBe('secondary');
    expect(container.querySelector('.border-t')?.className).toContain('border-lz-border');
    assertStudioClean(container);
  });
});
