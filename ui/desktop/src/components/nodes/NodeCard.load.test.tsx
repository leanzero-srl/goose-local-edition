import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { Mac } from '../leanzero-swarm/macs';
import { busyLoad } from '../leanzero-swarm/macLoad.fixtures';
import {
  NODE_FLASH,
  NODE_SPLIT,
  NODE_STUDIO,
  SELF_MAC,
  STUDIO_MAC,
  facts,
} from './nodeGlance.fixtures';

let mockMacs: Mac[] | null = null;
vi.mock('../leanzero-swarm/useMacs', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../leanzero-swarm/useMacs')>()),
  useMacsMaybe: () => (mockMacs ? { macs: mockMacs } : null),
}));

import { NodeCard, loadMacOf } from './NodeCard';
import { nodeGlance } from './nodeGlance';
import type { ResolvedNodeDef } from './model';

const SELF_LOADED: Mac = { ...SELF_MAC, load: busyLoad() };
const STUDIO_LOADED: Mac = { ...STUDIO_MAC, load: busyLoad({ jobs: [] }) };

function renderCard(node: ResolvedNodeDef) {
  return render(
    <IntlTestWrapper>
      <NodeCard node={node} glance={nodeGlance(node, facts())} usedBy={[]} onAction={() => {}} />
    </IntlTestWrapper>
  );
}

afterEach(() => {
  cleanup();
  mockMacs = null;
});

describe('NodeCard: the Mac load bars', () => {
  it('loadMacOf picks the one online Mac with load, never a split or provider', () => {
    const macs = [SELF_LOADED, STUDIO_LOADED];
    expect(loadMacOf({ kind: 'thisMac' }, macs)).toBe(SELF_LOADED);
    expect(loadMacOf({ kind: 'mac', name: STUDIO_MAC.name }, macs)).toBe(STUDIO_LOADED);
    expect(loadMacOf({ kind: 'split', count: 2 }, macs)).toBeNull();
    expect(loadMacOf({ kind: 'provider', name: 'OpenRouter' }, macs)).toBeNull();
    expect(loadMacOf(null, macs)).toBeNull();
    expect(loadMacOf({ kind: 'thisMac' }, [SELF_MAC])).toBeNull();
    expect(loadMacOf({ kind: 'thisMac' }, [{ ...SELF_LOADED, online: false }])).toBeNull();
  });

  it('a node on this Mac shows its three bars', () => {
    mockMacs = [SELF_LOADED, STUDIO_LOADED];
    renderCard(NODE_FLASH);
    const box = screen.getByTestId('node-mac-load');
    expect(
      within(box)
        .getAllByRole('meter')
        .map((m) => m.getAttribute('aria-valuenow'))
    ).toEqual(['62', '75', '50']);
  });

  it('a node on the Studio shows the Studio bars', () => {
    mockMacs = [SELF_LOADED, STUDIO_LOADED];
    renderCard(NODE_STUDIO);
    expect(
      within(screen.getByTestId('node-mac-load')).getByTestId('mac-load-jobs-count').textContent
    ).toBe('0');
  });

  it('a split node, a Mac without load, and no MacsProvider show none', () => {
    mockMacs = [SELF_LOADED, STUDIO_LOADED];
    renderCard(NODE_SPLIT);
    expect(screen.queryByTestId('node-mac-load')).toBeNull();
    cleanup();
    mockMacs = [SELF_MAC];
    renderCard(NODE_FLASH);
    expect(screen.queryByTestId('node-mac-load')).toBeNull();
    cleanup();
    mockMacs = null;
    renderCard(NODE_FLASH);
    expect(screen.queryByTestId('node-mac-load')).toBeNull();
  });
});
