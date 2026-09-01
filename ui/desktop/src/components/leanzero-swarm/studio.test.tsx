import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import {
  INPUT,
  StudioSelect,
  StudioSwitch,
  TEXTAREA,
  ToneBanner,
  WeightStepper,
  nodeHue,
} from './studio';

const OPTIONS = [
  { value: 'mlx', label: 'LeanZero MLX' },
  { value: 'zai', label: 'Z.ai' },
  { value: 'busy', label: 'studio-b', disabled: true },
];

function Gallery() {
  return (
    <div>
      <input className={INPUT} placeholder="p" />
      <textarea className={TEXTAREA} />
      <ToneBanner tone="err" label="Sign-in" text="rate limited" />
      <ToneBanner tone="warn" label="Deployment" text="no mesh" live testId="banner" />
      <ToneBanner tone="ok" label="Run" text="started" />
      <ToneBanner tone="accent" label="LeanZero MLX" text="one model" />
      <ToneBanner tone="stopped" label="awaiting fleet routing" text="saved" />
      <WeightStepper value={3} onChange={() => {}} label="node-a" />
      <StudioSwitch checked onChange={() => {}} aria-label="on" />
      <StudioSwitch checked={false} onChange={() => {}} aria-label="off" />
      <StudioSelect
        aria-label="Provider"
        options={OPTIONS}
        value={OPTIONS[0]}
        onChange={() => {}}
        placeholder="Pick…"
      />
      <StudioSelect
        aria-label="Loading"
        options={OPTIONS}
        value={null}
        onChange={() => {}}
        placeholder="Pick…"
        loading
      />
    </div>
  );
}

describe('leanzero-swarm studio compositions', () => {
  it('carry no banned pattern (rails, faded tints, native select) in any register', () => {
    const { container } = render(<Gallery />);
    assertStudioClean(container);
  });

  it('every class they emit compiles to a real rule against main.css', async () => {
    const { container } = render(<Gallery />);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(classes.length).toBeGreaterThan(40);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);

  it('the banner is an alert only for err, and renders the text VERBATIM in its own element', () => {
    render(<ToneBanner tone="err" label="Mesh" text="mesh joined but reported no IP" />);
    const alert = screen.getByRole('alert');
    expect(alert).toBeInTheDocument();
    expect(alert.getAttribute('data-tone')).toBe('err');
    expect(alert.style.backgroundColor).toBe('');
    expect(screen.getByText('mesh joined but reported no IP')).toBeInTheDocument();
    render(<ToneBanner tone="warn" label="Deployment" text="no mail" />);
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('no mail');
    expect(status.getAttribute('data-tone')).toBe('warn');
    expect(screen.getByText('Deployment').className).toContain('text-lz-warn');
  });

  it('the banner carries its tone as a toned label on every tone MlxEngineView raises', () => {
    render(
      <>
        <ToneBanner tone="accent" label="Remote" text="managing on studio" />
        <ToneBanner tone="err" label="Mount blocked" text="not enough memory" />
      </>
    );
    expect(screen.getByText('Remote').className).toContain('text-lz-accent');
    expect(screen.getByText('Remote').closest('[role]')?.getAttribute('role')).toBe('status');
    expect(screen.getByText('Mount blocked').closest('[role]')?.getAttribute('data-tone')).toBe(
      'err'
    );
  });

  it('the stepper clamps to 1–9 and names both buttons after the node', async () => {
    const onChange = vi.fn();
    const { rerender } = render(<WeightStepper value={9} onChange={onChange} label="zai-glm" />);
    await userEvent.click(screen.getByRole('button', { name: 'More work (zai-glm)' }));
    expect(onChange).toHaveBeenLastCalledWith(9);
    await userEvent.click(screen.getByRole('button', { name: 'Less work (zai-glm)' }));
    expect(onChange).toHaveBeenLastCalledWith(8);
    rerender(<WeightStepper value={1} onChange={onChange} label="zai-glm" />);
    await userEvent.click(screen.getByRole('button', { name: 'Less work (zai-glm)' }));
    expect(onChange).toHaveBeenLastCalledWith(1);
  });

  it('the switch is a role=switch button that toggles', async () => {
    const onChange = vi.fn();
    render(<StudioSwitch checked={false} onChange={onChange} aria-label="review" />);
    const sw = screen.getByRole('switch', { name: 'review' });
    expect(sw).toHaveAttribute('aria-checked', 'false');
    await userEvent.click(sw);
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it('the select is a combobox over a listbox: opens, lists options, disabled stays visible but unpickable, Escape closes', async () => {
    const onChange = vi.fn();
    render(
      <StudioSelect
        aria-label="Provider"
        options={OPTIONS}
        value={null}
        onChange={onChange}
        placeholder="Pick a provider…"
        optionTestId={(o) => `opt-${o.value}`}
      />
    );
    const trigger = screen.getByRole('combobox', { name: 'Provider' });
    expect(trigger).toHaveTextContent('Pick a provider…');
    expect(screen.queryByRole('listbox')).toBeNull();

    await userEvent.click(trigger);
    const options = screen.getAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual(['LeanZero MLX', 'Z.ai', 'studio-b']);
    expect(screen.getByTestId('opt-busy')).toBeDisabled();

    await userEvent.click(screen.getByTestId('opt-busy'));
    expect(onChange).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole('option', { name: 'Z.ai' }));
    expect(onChange).toHaveBeenCalledWith(OPTIONS[1]);
    expect(screen.queryByRole('listbox')).toBeNull();

    await userEvent.click(trigger);
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('listbox')).toBeNull();
  });

  it('the select is inert while loading and with no options', () => {
    render(
      <StudioSelect
        aria-label="Empty"
        options={[]}
        value={null}
        onChange={() => {}}
        placeholder="—"
      />
    );
    expect(screen.getByRole('combobox', { name: 'Empty' })).toBeDisabled();
  });

  it('nodeHue cycles the six-step ramp from 1', () => {
    expect([0, 1, 5, 6, 7].map(nodeHue)).toEqual([1, 2, 6, 1, 2]);
  });
});
