import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { createIntl } from 'react-intl';
import type { NodeServedTurnDto } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../i18n/test-utils';

const served = vi.hoisted(() => ({
  answer: null as null | { record?: NodeServedTurnDto } | Error,
}));
vi.mock('../../acp/nodes', () => ({
  nodesServedLast: async () => {
    if (served.answer instanceof Error) throw served.answer;
    return served.answer;
  },
}));
const loader = vi.hoisted(() => ({
  nodes: [] as {
    node: string;
    residency: { kind: string; phase?: string; demandedBy?: string[] };
  }[],
}));
vi.mock('../engineGlance/glanceStore', () => ({
  useGlanceNodes: () => ({
    kind: 'read',
    read: {
      nodes: [
        { def: { id: 'flash', name: 'Flash · this Mac' } },
        { def: { id: 'studio', name: '27B · Work’s Mac Studio' } },
        { def: { id: 'sonnet', name: 'Claude Sonnet' } },
      ],
    },
    residency: { nodes: loader.nodes },
  }),
}));

import { DelegateServedLine, delegateServedText } from './DelegateServedLine';

const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });
const NAMES = {
  flash: 'Flash · this Mac',
  studio: '27B · Work’s Mac Studio',
  sonnet: 'Claude Sonnet',
};
const record = (over: Partial<NodeServedTurnDto>): NodeServedTurnDto => ({
  node: 'flash',
  role: 'build',
  rank: 2,
  tried: [],
  atMs: 1,
  ...over,
});

afterEach(cleanup);

describe('the delegate card names the node it ran on (Q-359)', () => {
  it('on a node; loaded for it; or passed over the 1st, named with why', () => {
    expect(delegateServedText(intl, record({}), NAMES)).toBe('on Flash · this Mac');
    expect(delegateServedText(intl, record({ loadedMs: 98_000 }), NAMES)).toBe(
      'on Flash · this Mac · loaded for this delegate in 1m 38s'
    );
    expect(
      delegateServedText(
        intl,
        record({
          node: 'sonnet',
          reason: 'not connected',
          tried: [{ node: 'studio', reason: 'not connected' }],
        }),
        NAMES
      )
    ).toBe('on Claude Sonnet: 27B · Work’s Mac Studio can’t run (not connected)');
  });

  it('reads the delegate’s own record once it is done; no record says nothing; a failed read says so', async () => {
    served.answer = { record: record({}) };
    render(
      <IntlTestWrapper>
        <DelegateServedLine sessionId="sub-1" />
      </IntlTestWrapper>
    );
    expect(await screen.findByTestId('delegate-served-line')).toHaveTextContent(
      'on Flash · this Mac'
    );
    cleanup();
    served.answer = {};
    render(
      <IntlTestWrapper>
        <DelegateServedLine sessionId="sub-2" />
      </IntlTestWrapper>
    );
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByTestId('delegate-served-line')).toBeNull();
    cleanup();
    served.answer = new Error('goosed is gone');
    render(
      <IntlTestWrapper>
        <DelegateServedLine sessionId="sub-3" />
      </IntlTestWrapper>
    );
    expect(await screen.findByTestId('delegate-served-line')).toHaveTextContent(
      'The node this delegate ran on could not be read: goosed is gone'
    );
  });

  it('Q-382: while the loader loads a node FOR this delegate, says so with the phase', async () => {
    served.answer = { record: record({ node: 'studio', loadedMs: 98_000 }) };
    loader.nodes.splice(0, loader.nodes.length, {
      node: 'studio',
      residency: { kind: 'loading', phase: 'warming', demandedBy: ['sub-7', 'sub-8'] },
    });
    const { rerender } = render(
      <IntlTestWrapper>
        <DelegateServedLine sessionId="sub-8" />
      </IntlTestWrapper>
    );
    expect(screen.getByTestId('delegate-loading-line')).toHaveTextContent(
      'Loading 27B · Work’s Mac Studio for this delegate: Warming up'
    );
    expect(screen.queryByTestId('delegate-served-line')).toBeNull();
    // The load ends: the delegate's record is read then, naming where it ran.
    loader.nodes.splice(0, loader.nodes.length);
    rerender(
      <IntlTestWrapper>
        <DelegateServedLine sessionId="sub-8" />
      </IntlTestWrapper>
    );
    expect(await screen.findByTestId('delegate-served-line')).toHaveTextContent(
      'on 27B · Work’s Mac Studio · loaded for this delegate in 1m 38s'
    );
    expect(screen.queryByTestId('delegate-loading-line')).toBeNull();
  });
});
