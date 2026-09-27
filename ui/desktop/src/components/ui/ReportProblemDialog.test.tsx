import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { ReportProblemDialog } from './ReportProblemDialog';
import { getDiagnosticsReport } from '../../acp/diagnostics';
import { LEANZERO_DISCORD_INVITE_URL } from '../../branding';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import type { DiagnosticsReport } from '../../types/diagnostics';

vi.mock('../../acp/diagnostics', () => ({ getDiagnosticsReport: vi.fn() }));

/**
 * A summary report as the engine returns it — plus the fields a FULL report carries, laced with
 * secrets, so the test proves the attachment reads only the named system fields whatever arrives.
 */
const REPORT = {
  schemaVersion: 1,
  generatedAt: '2026-09-27T20:00:00Z',
  level: 'summary',
  errors: [],
  extensions: { enabled: ['developer'] },
  logs: { llm: [{ path: '/Users/ana/.log', content: 'SECRET-LOG', truncated: false }] },
  prompts: [],
  scheduledRecipes: [],
  config: {
    configPath: '/Users/ana/config.yaml',
    configYaml: 'OPENAI_API_KEY: SECRET-KEY',
    truncated: false,
  },
  session: { messages: ['SECRET-CHAT'] },
  system: {
    app_version: '1.9.0',
    architecture: 'aarch64',
    enabled_extensions: ['developer'],
    model: 'qwen3.6-27b',
    os: 'macos',
    os_version: '26.6.1',
    provider: 'lmstudio',
  },
} as unknown as DiagnosticsReport;

const DETAILS = [
  'Goose Swarm desktop: 3.0.39',
  'goose engine: 1.9.0',
  'OS: macos 26.6.1 (aarch64)',
  'Provider: lmstudio',
  'Model: qwen3.6-27b',
  'Extensions: developer',
].join('\n');

type ElectronStub = {
  openExternal: ReturnType<typeof vi.fn>;
  getVersion: ReturnType<typeof vi.fn>;
  sendProblemReport: ReturnType<typeof vi.fn>;
};
const electron = () => window.electron as unknown as ElectronStub;

beforeEach(() => {
  Object.assign(window.electron, {
    openExternal: vi.fn(async () => {}),
    getVersion: vi.fn(() => '3.0.39'),
    sendProblemReport: vi.fn(async () => ({ ok: true })),
  });
  vi.mocked(getDiagnosticsReport).mockResolvedValue(REPORT);
});

afterEach(() => {
  vi.mocked(getDiagnosticsReport).mockReset();
});

const mount = (onClose = vi.fn()) =>
  render(
    <IntlTestWrapper>
      <ReportProblemDialog isOpen onClose={onClose} sessionId="s1" />
    </IntlTestWrapper>
  );

const describeProblem = (text = 'The chat froze after I sent a long message.') =>
  fireEvent.change(screen.getByLabelText('What went wrong?'), { target: { value: text } });
const typeEmail = (value: string) =>
  fireEvent.change(screen.getByLabelText('Your email'), { target: { value } });
const click = async (name: string | RegExp) => {
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name }));
  });
};
const tickAttach = async () => {
  await act(async () => {
    fireEvent.click(screen.getByRole('checkbox', { name: 'Attach system details' }));
  });
};

describe('Report a problem — a real dialog (Q-21, Q-192)', () => {
  it('is a modal named by its title that says where the report goes; Escape closes it', () => {
    const onClose = vi.fn();
    mount(onClose);
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveAccessibleName('Report a problem');
    expect(screen.getByText('Your report goes to office@leanzero.net.')).toBeInTheDocument();
    expect(screen.queryByText(/GitHub/)).toBeNull();
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
    // The panel itself (Radix's focus-guard spans outside it carry their own inline opacity).
    assertStudioClean(dialog);
  });

  it('dims the app at half strength — never solid black (bg-opacity-* does nothing on Tailwind 4)', () => {
    mount();
    const layer = Array.from(document.querySelectorAll<HTMLElement>('.fixed')).find((el) =>
      el.className.includes('inset-0')
    );
    expect(layer!.className).toContain('bg-black/50');
  });

  it('every class the dialog emits compiles to a real rule against main.css', async () => {
    mount();
    await tickAttach();
    await screen.findByTestId('report-problem-preview-text');
    const classes = allClasses(document.body).filter(
      (c) => !c.startsWith('lucide') && c !== 'bg-black/50' && !c.startsWith('data-[')
    );
    expect(classes.length).toBeGreaterThan(20);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});

describe('Report a problem — validation', () => {
  it('refuses a description under 10 characters, and sends nothing', async () => {
    mount();
    describeProblem('broken');
    typeEmail('ana@example.com');
    await click('Send');
    expect(screen.getByRole('alert').textContent).toBe(
      'Describe the problem in at least 10 characters.'
    );
    expect(electron().sendProblemReport).not.toHaveBeenCalled();
  });

  it('SEND needs a reply address and says the mail app does not', async () => {
    mount();
    describeProblem();
    await click('Send');
    expect(screen.getByRole('alert').textContent).toBe(
      'Add your email so LeanZero can reply, or send it from your mail app.'
    );
    typeEmail('ana@example');
    await click('Send');
    expect(screen.getByRole('alert').textContent).toBe('That email address does not look right.');
    expect(electron().sendProblemReport).not.toHaveBeenCalled();
  });
});

describe('Report a problem — attaching system details is opt-in and previewed exactly', () => {
  it('reads nothing until ticked, then shows the six lines it will attach — no chats, logs or config', async () => {
    mount();
    expect(screen.getByRole('checkbox', { name: 'Attach system details' })).toHaveAttribute(
      'aria-checked',
      'false'
    );
    expect(getDiagnosticsReport).not.toHaveBeenCalled();
    expect(screen.queryByTestId('report-problem-preview')).toBeNull();

    await tickAttach();
    expect(getDiagnosticsReport).toHaveBeenCalledWith('s1', 'summary');
    const preview = await screen.findByTestId('report-problem-preview-text');
    expect(preview.textContent).toBe(DETAILS);
    expect(screen.getByText('Exactly what will be attached:')).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/SECRET|config\.yaml|\/Users\/ana/);
  });

  it('SEND carries exactly the previewed text', async () => {
    mount();
    describeProblem();
    typeEmail('ana@example.com');
    await tickAttach();
    const shown = (await screen.findByTestId('report-problem-preview-text')).textContent;
    await click('Send');
    expect(electron().sendProblemReport).toHaveBeenCalledWith({
      description: 'The chat froze after I sent a long message.',
      email: 'ana@example.com',
      attachment: shown,
      desktopVersion: '3.0.39',
    });
  });

  it('ticked then unticked: the preview goes and nothing is attached', async () => {
    mount();
    describeProblem();
    typeEmail('ana@example.com');
    await tickAttach();
    await screen.findByTestId('report-problem-preview-text');
    await tickAttach();
    expect(screen.queryByTestId('report-problem-preview')).toBeNull();
    await click('Send');
    expect(electron().sendProblemReport.mock.calls[0][0].attachment).toBeNull();
  });

  it('a report sent without ticking carries no attachment', async () => {
    mount();
    describeProblem();
    typeEmail('ana@example.com');
    await click('Send');
    expect(electron().sendProblemReport.mock.calls[0][0].attachment).toBeNull();
    expect(getDiagnosticsReport).not.toHaveBeenCalled();
  });

  it('when the details cannot be read it says so, unticks, and attaches nothing', async () => {
    vi.mocked(getDiagnosticsReport).mockRejectedValue(new Error('engine not running'));
    mount();
    describeProblem();
    typeEmail('ana@example.com');
    await tickAttach();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not read system details (engine not running). Nothing will be attached.'
    );
    expect(screen.getByRole('checkbox', { name: 'Attach system details' })).toHaveAttribute(
      'aria-checked',
      'false'
    );
    await click('Send');
    expect(electron().sendProblemReport.mock.calls[0][0].attachment).toBeNull();
  });
});

describe('Report a problem — SEND', () => {
  it('success replaces the form with a confirmation naming the reply address', async () => {
    const onClose = vi.fn();
    mount(onClose);
    describeProblem();
    typeEmail('ana@example.com');
    await click('Send');
    const sent = await screen.findByTestId('report-problem-sent');
    expect(sent).toHaveTextContent('Report sent');
    expect(sent).toHaveTextContent('will reply to ana@example.com');
    expect(screen.queryByLabelText('What went wrong?')).toBeNull();
    await click('Done');
    expect(onClose).toHaveBeenCalled();
  });

  it('failure keeps the report, quotes the server and offers the mail app', async () => {
    electron().sendProblemReport.mockResolvedValue({
      ok: false,
      reason: 'http',
      status: 503,
      message: 'Contact service is not configured',
    });
    mount();
    describeProblem();
    typeEmail('ana@example.com');
    await click('Send');
    expect(screen.getByRole('alert').textContent).toBe(
      'It did not go through: Contact service is not configured (HTTP 503). Your report is still here. Try again, or send it from your mail app.'
    );
    expect(screen.getByLabelText('What went wrong?')).toHaveValue(
      'The chat froze after I sent a long message.'
    );
    expect(screen.getByRole('button', { name: 'Use my mail app' })).toBeEnabled();
    expect(screen.queryByTestId('report-problem-sent')).toBeNull();
  });

  it('a network failure is named too', async () => {
    electron().sendProblemReport.mockResolvedValue({
      ok: false,
      reason: 'network',
      message: 'fetch failed',
    });
    mount();
    describeProblem();
    typeEmail('ana@example.com');
    await click('Send');
    expect(screen.getByRole('alert').textContent).toContain('It did not go through: fetch failed.');
  });
});

describe('Report a problem — the mail app and the Discord', () => {
  it('the mail app opens a prefilled email to office@ with no email typed, then says it is not sent yet', async () => {
    mount();
    describeProblem();
    await click('Use my mail app');
    expect(electron().openExternal).toHaveBeenCalledTimes(1);
    const url = new URL(electron().openExternal.mock.calls[0][0] as string);
    expect(url.protocol).toBe('mailto:');
    expect(url.pathname).toBe('office@leanzero.net');
    const params = new URLSearchParams(url.search);
    expect(params.get('subject')).toBe('Goose Swarm problem report');
    expect(params.get('body')).toBe('The chat froze after I sent a long message.');
    const mailed = await screen.findByTestId('report-problem-mailed');
    expect(mailed).toHaveTextContent('It is not sent until you send it from there.');
    expect(electron().sendProblemReport).not.toHaveBeenCalled();
  });

  it('the mail app also refuses a too-short description', async () => {
    mount();
    describeProblem('help');
    await click('Use my mail app');
    expect(electron().openExternal).not.toHaveBeenCalled();
    expect(screen.getByRole('alert').textContent).toBe(
      'Describe the problem in at least 10 characters.'
    );
  });

  it('Join the LeanZero Discord opens the invite leanzero.net links', async () => {
    mount();
    await click('Join the LeanZero Discord');
    await waitFor(() =>
      expect(electron().openExternal).toHaveBeenCalledWith(LEANZERO_DISCORD_INVITE_URL)
    );
    expect(LEANZERO_DISCORD_INVITE_URL).toBe('https://discord.gg/RvYbd9qEUT');
  });
});
