/**
 * NodeCard's "Free up memory" entry: only on an mlx card that cannot run because the model is
 * short of memory, pointed at the Mac the card was placed on, and only while the local edition
 * and GOOSE_MEMFREE_ENABLED allow it.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { Mac } from '../leanzero-swarm/macs';
import {
  NODE_FLASH,
  NODE_LOCAL_27B,
  NODE_SPLIT,
  SELF_MAC,
  STUDIO_MAC,
  facts,
} from './nodeGlance.fixtures';

let mockMacs: Mac[] | null = null;
vi.mock('../leanzero-swarm/useMacs', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../leanzero-swarm/useMacs')>()),
  useMacsMaybe: () => (mockMacs ? { macs: mockMacs } : null),
}));

let mockEnabled: unknown = undefined;
const mockRead = vi.fn(async () => mockEnabled);
vi.mock('../../acp/config', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/config')>()),
  acpReadConfig: () => mockRead(),
}));

let mockLocal = true;
vi.mock('../../contexts/EditionContext', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../contexts/EditionContext')>()),
  useEditionIsLocal: () => mockLocal,
}));

import { NodeCard, memfreeTargetOf } from './NodeCard';
import { nodeGlance, type NodeGlance } from './nodeGlance';
import type { ResolvedNodeDef } from './model';
import { forgetMemfreeEnabled } from '../memfree/useMemfreeEnabled';

function renderCard(node: ResolvedNodeDef, f = facts()) {
  return render(
    <IntlTestWrapper>
      <NodeCard node={node} glance={nodeGlance(node, f)} usedBy={[]} onAction={() => {}} />
    </IntlTestWrapper>
  );
}

/** Let the config read settle before asserting the button is absent. */
async function settled() {
  await waitFor(() => expect(mockRead).toHaveBeenCalled());
  await new Promise((r) => setTimeout(r, 0));
}

afterEach(() => {
  cleanup();
  mockMacs = null;
  mockEnabled = undefined;
  mockLocal = true;
  mockRead.mockClear();
  forgetMemfreeEnabled();
});

describe('memfreeTargetOf', () => {
  const short = nodeGlance(NODE_LOCAL_27B, facts());
  const macs = [SELF_MAC, STUDIO_MAC];

  it('a short card on this Mac points at this Mac (no node id)', () => {
    expect(short.state).toBe('cantRun');
    expect(memfreeTargetOf(short, macs)).toEqual({ target: null, mac: SELF_MAC });
  });

  it('a short card placed on an online peer points at its Link node id', () => {
    const onStudio: NodeGlance = { ...short, where: { kind: 'mac', name: STUDIO_MAC.name } };
    expect(memfreeTargetOf(onStudio, macs)).toEqual({ target: STUDIO_MAC.nodeId, mac: STUDIO_MAC });
  });

  it('an offline or unknown peer gets no entry', () => {
    const onStudio: NodeGlance = { ...short, where: { kind: 'mac', name: STUDIO_MAC.name } };
    expect(memfreeTargetOf(onStudio, [SELF_MAC, { ...STUDIO_MAC, online: false }])).toBeNull();
    expect(memfreeTargetOf(onStudio, [SELF_MAC])).toBeNull();
  });

  it('cantRun for another reason, or a card that runs, gets no entry', () => {
    const offline = nodeGlance(
      NODE_SPLIT,
      facts({ macs: [SELF_MAC, { ...STUDIO_MAC, online: false }] })
    );
    expect(offline.state).toBe('cantRun');
    expect(memfreeTargetOf(offline, macs)).toBeNull();
    expect(memfreeTargetOf(nodeGlance(NODE_FLASH, facts()), macs)).toBeNull();
  });
});

describe('NodeCard: the Free up memory entry', () => {
  it('shows on a card that is short of memory', async () => {
    mockMacs = [SELF_MAC, STUDIO_MAC];
    renderCard(NODE_LOCAL_27B);
    const button = await screen.findByTestId('node-free-memory');
    expect(button.textContent).toContain('Free up memory');
  });

  it('is absent on a card that fits', async () => {
    mockMacs = [SELF_MAC, STUDIO_MAC];
    renderCard(NODE_FLASH);
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByTestId('node-free-memory')).toBeNull();
    expect(mockRead).not.toHaveBeenCalled();
  });

  it('is absent when GOOSE_MEMFREE_ENABLED is false', async () => {
    mockMacs = [SELF_MAC, STUDIO_MAC];
    mockEnabled = false;
    renderCard(NODE_LOCAL_27B);
    await settled();
    expect(screen.queryByTestId('node-free-memory')).toBeNull();
  });

  it('is absent outside the local edition', async () => {
    mockMacs = [SELF_MAC, STUDIO_MAC];
    mockLocal = false;
    renderCard(NODE_LOCAL_27B);
    await settled();
    expect(screen.queryByTestId('node-free-memory')).toBeNull();
  });
});
