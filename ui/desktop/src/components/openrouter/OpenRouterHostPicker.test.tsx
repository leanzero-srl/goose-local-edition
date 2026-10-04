import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  acpListOpenRouterHosts,
  acpProbeOpenRouterHost,
  acpReadOpenRouterPin,
  acpSetOpenRouterPin,
  type OpenRouterProbe,
} from '../../acp/openrouter';
import { OpenRouterHostPicker } from './OpenRouterHostPicker';

vi.mock('../../acp/openrouter', () => ({
  acpReadOpenRouterPin: vi.fn(),
  acpSetOpenRouterPin: vi.fn(),
  acpListOpenRouterHosts: vi.fn(),
  acpProbeOpenRouterHost: vi.fn(),
}));

const MODEL = 'qwen/qwen3.8-27b';
const WAFER_PIN = {
  tag: 'wafer',
  raw: '{"provider":{"order":["wafer"],"allow_fallbacks":false}}',
};
const NOVITA_PIN = {
  tag: 'novita',
  raw: '{"provider":{"order":["novita"],"allow_fallbacks":false}}',
};

const hosts = [
  {
    tag: 'wafer',
    providerName: 'Wafer',
    quantization: 'fp8',
    contextLength: 262144,
    supportsTools: true,
    uptimeLast30m: 99.2,
  },
  { tag: 'novita', providerName: 'Novita', contextLength: 262144, supportsTools: true },
  { tag: 'parasail/fp8', providerName: 'Parasail', quantization: 'fp8', supportsTools: true },
  { tag: 'alibaba', providerName: 'Alibaba', supportsTools: true },
  { tag: 'deepinfra/bf16', providerName: 'DeepInfra', supportsTools: false },
];

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** The full-width row under a host's result that carries its error text. */
const subRow = (tag: string) =>
  document.querySelector(`[data-testid="lz-sub-row"][data-key="${tag}"]`);

const probe = (tag: string, extra: Partial<OpenRouterProbe>): OpenRouterProbe =>
  ({ tag, seconds: 10, toolCall: false, ...extra }) as OpenRouterProbe;

beforeEach(() => {
  vi.mocked(acpReadOpenRouterPin).mockResolvedValue(WAFER_PIN);
  vi.mocked(acpListOpenRouterHosts).mockResolvedValue({ model: MODEL, hosts, untagged: [] });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function openPicker() {
  const trigger = await screen.findByRole('combobox', { name: 'OpenRouter host' });
  await waitFor(() => expect(trigger).toBeEnabled());
  await waitFor(() => expect(acpListOpenRouterHosts).toHaveBeenCalledWith(MODEL));
  await screen.findByText(/Not offered, no tool support: DeepInfra \(deepinfra\/bf16\)/);
  fireEvent.click(trigger);
  return screen.getByRole('listbox', { name: 'OpenRouter host' });
}

it('reflects the saved pin on open and offers Any host plus every host with tool support', async () => {
  render(<OpenRouterHostPicker model={MODEL} />);
  const trigger = await screen.findByRole('combobox', { name: 'OpenRouter host' });
  await waitFor(() =>
    expect(trigger).toHaveTextContent('Wafer · fp8 · 262K context · 99.2% uptime')
  );
  expect(
    screen.getByText(/Every OpenRouter request runs on Wafer \(wafer\), with no fallback/)
  ).toBeInTheDocument();
  const listbox = await openPicker();
  const options = within(listbox).getAllByRole('option');
  expect(options.map((o) => o.textContent)).toEqual([
    'Any host (OpenRouter routes)',
    'Wafer · fp8 · 262K context · 99.2% uptime',
    'Novita · 262K context',
    'Parasail · fp8',
    'Alibaba',
  ]);
  expect(options[1]).toHaveAttribute('aria-selected', 'true');
});

it('choosing a host writes that pin, and Any host removes it', async () => {
  vi.mocked(acpSetOpenRouterPin).mockResolvedValueOnce(NOVITA_PIN).mockResolvedValueOnce({});
  render(<OpenRouterHostPicker model={MODEL} />);
  let listbox = await openPicker();
  fireEvent.click(within(listbox).getByRole('option', { name: 'Novita · 262K context' }));
  await waitFor(() => expect(acpSetOpenRouterPin).toHaveBeenCalledWith('novita'));
  const trigger = screen.getByRole('combobox', { name: 'OpenRouter host' });
  await waitFor(() => expect(trigger).toHaveTextContent('Novita · 262K context'));

  fireEvent.click(trigger);
  listbox = screen.getByRole('listbox', { name: 'OpenRouter host' });
  fireEvent.click(within(listbox).getByRole('option', { name: 'Any host (OpenRouter routes)' }));
  await waitFor(() => expect(acpSetOpenRouterPin).toHaveBeenLastCalledWith(null));
  await waitFor(() => expect(trigger).toHaveTextContent('Any host (OpenRouter routes)'));
  expect(screen.getByText('OpenRouter picks the host for every request.')).toBeInTheDocument();
});

it('tests every tool host at once, shows each one running, and one failure never blocks the rest', async () => {
  const calls = new Map<string, ReturnType<typeof deferred<OpenRouterProbe>>>();
  vi.mocked(acpProbeOpenRouterHost).mockImplementation((_model, tag) => {
    const d = deferred<OpenRouterProbe>();
    calls.set(tag, d);
    return d.promise;
  });
  render(<OpenRouterHostPicker model={MODEL} />);
  await openPicker();
  fireEvent.keyDown(screen.getByRole('listbox', { name: 'OpenRouter host' }), { key: 'Escape' });

  fireEvent.click(screen.getByRole('button', { name: 'Test hosts' }));
  // All four requests left before any answered — in parallel, never one after another.
  expect([...calls.keys()].sort()).toEqual(['alibaba', 'novita', 'parasail/fp8', 'wafer']);
  expect(acpProbeOpenRouterHost).toHaveBeenCalledWith(MODEL, 'wafer');
  expect(screen.getAllByLabelText('Testing')).toHaveLength(4);
  expect(screen.getByRole('button', { name: /Testing… 0 of 4 answered/ })).toBeDisabled();

  calls.get('alibaba')!.reject(new Error('backend disconnected'));
  calls.get('parasail/fp8')!.resolve(
    probe('parasail/fp8', {
      tokensPerSecond: 89.1,
      finishReason: 'length',
      completionTokens: 3000,
    })
  );
  await waitFor(() => expect(screen.getAllByLabelText('Testing')).toHaveLength(2));
  expect(screen.getByText('backend disconnected')).toBeInTheDocument();

  calls
    .get('novita')!
    .resolve(
      probe('novita', { tokensPerSecond: 56.7, toolCall: true, finishReason: 'tool_calls' })
    );
  calls
    .get('wafer')!
    .resolve(probe('wafer', { tokensPerSecond: 79.4, toolCall: true, finishReason: 'tool_calls' }));
  await waitFor(() => expect(screen.queryAllByLabelText('Testing')).toHaveLength(0));

  const table = screen.getByRole('table', { name: 'Host test results' });
  const order = within(table)
    .getAllByRole('row')
    .filter((row) => row.getAttribute('data-testid')?.startsWith('openrouter-probe-'))
    .map((row) => row.getAttribute('data-key'));
  expect(order).toEqual(['wafer', 'novita', 'parasail/fp8', 'alibaba']);
  expect(within(screen.getByTestId('openrouter-probe-alibaba')).getByText('failed')).toBeVisible();
  expect(subRow('alibaba')).toHaveTextContent('backend disconnected');
  expect(subRow('wafer')).toBeNull();
  const wafer = screen.getByTestId('openrouter-probe-wafer');
  expect(within(wafer).getByText('Recommended')).toBeInTheDocument();
  expect(within(wafer).getByText('79.4')).toBeInTheDocument();
  expect(within(wafer).getByText('yes')).toBeInTheDocument();
  expect(
    within(screen.getByTestId('openrouter-probe-parasail/fp8')).getByText('no')
  ).toBeInTheDocument();
  expect(
    screen.getByText('4 hosts tested on qwen/qwen3.8-27b', { exact: false })
  ).toBeInTheDocument();
});

it('shows a host refusal verbatim with its status, and a result row can be chosen as the host', async () => {
  const age = 'This model requires 18+ age confirmation. Visit https://openrouter.ai/settings';
  vi.mocked(acpProbeOpenRouterHost).mockImplementation(async (_model, tag) =>
    tag === 'novita'
      ? probe('novita', { tokensPerSecond: 56.7, toolCall: true, finishReason: 'tool_calls' })
      : probe(tag, { error: age, httpStatus: 403, seconds: 0.4 })
  );
  vi.mocked(acpSetOpenRouterPin).mockResolvedValue(NOVITA_PIN);
  render(<OpenRouterHostPicker model={MODEL} />);
  await openPicker();
  fireEvent.keyDown(screen.getByRole('listbox', { name: 'OpenRouter host' }), { key: 'Escape' });
  fireEvent.click(screen.getByRole('button', { name: 'Test hosts' }));

  const wafer = await screen.findByTestId('openrouter-probe-wafer');
  await waitFor(() => expect(subRow('wafer')).toHaveTextContent(`403: ${age}`));
  expect(within(wafer).getByText('HTTP 403')).toBeInTheDocument();
  expect(within(wafer).getByText('Pinned')).toBeInTheDocument();
  const novita = screen.getByTestId('openrouter-probe-novita');
  expect(within(novita).getByText('Recommended')).toBeInTheDocument();
  fireEvent.click(within(novita).getByRole('button', { name: 'Use' }));
  await waitFor(() => expect(acpSetOpenRouterPin).toHaveBeenCalledWith('novita'));
  await waitFor(() =>
    expect(
      within(screen.getByTestId('openrouter-probe-novita')).getByRole('button')
    ).toHaveTextContent('In use')
  );
});

it('says loudly when the saved pin names a host that does not serve this model', async () => {
  vi.mocked(acpReadOpenRouterPin).mockResolvedValue({
    tag: 'xiaomi',
    raw: '{"provider":{"order":["xiaomi"],"allow_fallbacks":false}}',
  });
  render(<OpenRouterHostPicker model={MODEL} />);
  expect(
    await screen.findByText(/The pin names xiaomi, which does not serve qwen\/qwen3\.8-27b/)
  ).toBeInTheDocument();
  expect(screen.getByRole('combobox', { name: 'OpenRouter host' })).toHaveTextContent(
    'xiaomi (saved pin — not a tool host for this model)'
  );
});

it('re-reads the pin when the window regains focus, so a pin the benchmark driver rewrote shows', async () => {
  render(<OpenRouterHostPicker model={MODEL} />);
  const trigger = await screen.findByRole('combobox', { name: 'OpenRouter host' });
  await waitFor(() => expect(trigger).toHaveTextContent('Wafer'));
  await screen.findByText(/Not offered, no tool support/);
  vi.mocked(acpReadOpenRouterPin).mockResolvedValue(NOVITA_PIN);
  fireEvent.focus(window);
  await waitFor(() => expect(trigger).toHaveTextContent('Novita · 262K context'));
});

it('names a listing failure and still lets the pin be removed', async () => {
  vi.mocked(acpListOpenRouterHosts).mockRejectedValue(new Error('OpenRouter is not set up'));
  vi.mocked(acpSetOpenRouterPin).mockResolvedValue({});
  render(<OpenRouterHostPicker model={MODEL} />);
  expect(await screen.findByRole('alert')).toHaveTextContent(
    `OpenRouter's hosts for ${MODEL} could not be listed: OpenRouter is not set up`
  );
  expect(screen.getByRole('button', { name: 'Test hosts' })).toBeDisabled();
  const trigger = screen.getByRole('combobox', { name: 'OpenRouter host' });
  fireEvent.click(trigger);
  fireEvent.click(screen.getByRole('option', { name: 'Any host (OpenRouter routes)' }));
  await waitFor(() => expect(acpSetOpenRouterPin).toHaveBeenCalledWith(null));
});

it('renders nothing and lists nothing without a model id', async () => {
  const { container } = render(<OpenRouterHostPicker model="  " />);
  await new Promise((resolve) => setTimeout(resolve, 600));
  expect(container).toBeEmptyDOMElement();
  expect(acpListOpenRouterHosts).not.toHaveBeenCalled();
});
