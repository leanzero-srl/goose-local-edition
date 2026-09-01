import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import {
  AMBER,
  AZURE,
  Chip,
  DownloadProgressRow,
  GREEN,
  RED,
  SLATE,
  SolidBanner,
  WeightStepper,
  toneForColor,
} from './primitives';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

// The adapter over the Studio primitives: the old names and signatures, every visual a token.
// These pin the two chip registers, the colour→tone join, and that nothing here is hand-coloured.
describe('leanzero primitives — chip registers', () => {
  it('a quiet chip is the Studio outline in ink-3 with NO fill', () => {
    const { getByText } = render(<Chip quiet>lmstudio-community</Chip>);
    const chip = getByText('lmstudio-community');
    expect(chip.style.backgroundColor).toBe('');
    expect(chip.className).toContain('border-lz-border-strong');
    expect(chip.className).toContain('text-lz-ink-3');
    // Metadata is never shouted: no uppercase, no letter-spacing.
    expect(chip.className).not.toMatch(/uppercase|tracking-/);
  });

  it('a palette colour becomes the matching status fill — never an inline style', () => {
    const { getByText } = render(
      <>
        <Chip color={GREEN} ink="#ffffff">
          mounted
        </Chip>
        <Chip color={AMBER}>busy</Chip>
        <Chip tone="err">failed</Chip>
      </>
    );
    const mounted = getByText('mounted');
    expect(mounted.style.backgroundColor).toBe('');
    expect(mounted.className).toContain('bg-lz-ok-solid');
    expect(mounted.getAttribute('data-tone')).toBe('ok');
    expect(getByText('busy').className).toContain('bg-lz-warn-solid');
    expect(getByText('failed').className).toContain('bg-lz-err-solid');
  });

  it('a chip with no colour falls through to the quiet register rather than a transparent fill', () => {
    const { getByText } = render(<Chip>4-bit</Chip>);
    const chip = getByText('4-bit');
    expect(chip.style.backgroundColor).toBe('');
    expect(chip.className).not.toMatch(/(^|\s)bg-/);
  });

  it('every palette constant resolves through a theme token and reads back as its tone', () => {
    for (const c of [AZURE, GREEN, RED, SLATE]) {
      expect(c).toMatch(/^var\(--color-(action-solid|status-[a-z]+-solid), #[0-9a-f]{6}\)$/);
      expect(c).not.toContain('--color-node-');
    }
    expect(toneForColor(AZURE)).toBe('accent');
    expect(toneForColor(GREEN)).toBe('ok');
    expect(toneForColor(RED)).toBe('err');
    expect(toneForColor(SLATE)).toBe('stopped');
    expect(toneForColor(AMBER)).toBe('warn');
    expect(toneForColor('#e5484d')).toBeNull();
  });
});

describe('leanzero primitives — banner, stepper, download row', () => {
  it('a banner carries its tone as a dot + toned label; red is an alert, the rest a status', () => {
    const { getByRole, getByText } = render(
      <>
        <SolidBanner color={RED} label="Mount blocked" text="not enough memory" />
        <SolidBanner color={AMBER} label="Restart required" text="remount to apply" />
        <SolidBanner tone="accent" label="Remote" text="managing on studio" />
      </>
    );
    const alert = getByRole('alert');
    expect(alert.getAttribute('data-tone')).toBe('err');
    expect(alert).toHaveTextContent('not enough memory');
    expect(getByText('Restart required').className).toContain('text-lz-warn');
    expect(getByText('Remote').className).toContain('text-lz-accent');
    expect(getByText('Remote').closest('[role]')?.getAttribute('role')).toBe('status');
  });

  it('a colour outside the palette on a banner reads as err (the failed-banner caller)', () => {
    const { getByRole } = render(<SolidBanner color="#e5484d" label="failed" text="boom" />);
    expect(getByRole('alert').getAttribute('data-tone')).toBe('err');
  });

  it('the weight stepper keeps its labels and clamps to 1–9', () => {
    const calls: number[] = [];
    const { getByLabelText } = render(
      <WeightStepper value={9} onChange={(v) => calls.push(v)} label="node-a" />
    );
    getByLabelText('More work (node-a)').click();
    getByLabelText('Less work (node-a)').click();
    expect(calls).toEqual([9, 8]);
  });

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
        <Chip quiet>q</Chip>
        <Chip color={GREEN}>g</Chip>
        <Chip color={AMBER}>a</Chip>
        <Chip color={SLATE}>s</Chip>
        <Chip color={AZURE}>z</Chip>
        <SolidBanner color={RED} label="l" text="t" />
        <SolidBanner color={AMBER} label="l" text="t" action={<button type="button">x</button>} />
        <WeightStepper value={3} onChange={noop} />
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
