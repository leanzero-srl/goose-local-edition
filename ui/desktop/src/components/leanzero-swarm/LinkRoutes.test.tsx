import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { LinkRoutesPanel, routeLine } from './LinkRoutes';
import type { LinkRoute } from '../../acp/leanzero-link';

const HOST = 'worksmacstudio.tailfc4700.ts.net';
const AT = '2026-09-26T15:39:19Z';

describe('routeLine (Q-137)', () => {
  it('a tailnet road names the address', () => {
    expect(routeLine({ host: HOST, path: 'tailnet', ip: '100.122.51.13', decidedAt: AT })).toEqual({
      tone: 'ok',
      text: 'Over your tailnet (100.122.51.13)',
    });
  });

  it('a dead Funnel says which Mac to restart Tailscale on — never "not connected"', () => {
    const line = routeLine({
      host: HOST,
      path: 'public',
      reason: 'MagicDNS answered 185.40.234.198',
      decidedAt: AT,
      lastFailure: 'error sending request: tls handshake eof',
    });
    expect(line.tone).toBe('err');
    expect(line.text).toBe(
      'The public Tailscale address of worksmacstudio stopped answering — restart Tailscale on worksmacstudio'
    );
  });

  it('a working Funnel road is a warning, a non-Tailscale server is plain', () => {
    expect(routeLine({ host: HOST, path: 'public', reason: 'x', decidedAt: AT }).tone).toBe('warn');
    expect(
      routeLine({ host: 'controlplane.tailscale.com', path: 'public', reason: 'x', decidedAt: AT })
    ).toEqual({ tone: 'secondary', text: 'Public address' });
  });

  it('a failed tailnet road is its own failure, not a Funnel one', () => {
    const line = routeLine({
      host: HOST,
      path: 'tailnet',
      ip: '100.122.51.13',
      decidedAt: AT,
      lastFailure: 'connection refused',
    });
    expect(line.tone).toBe('err');
    expect(line.text).not.toContain('Funnel');
    expect(line.text).not.toContain('public');
  });
});

describe('LinkRoutesPanel', () => {
  it('shows both roads and puts the measured evidence behind Details', () => {
    const control: LinkRoute = {
      host: HOST,
      path: 'public',
      reason: "this Mac's MagicDNS (100.100.100.100) did not answer",
      decidedAt: AT,
      lastFailure: 'accepted the connection and closed it without sending a byte',
    };
    render(
      <LinkRoutesPanel
        routes={{
          worker: { host: HOST, path: 'tailnet', ip: '100.122.51.13', decidedAt: AT },
          control,
        }}
      />
    );
    expect(screen.getByTestId('link-route-worker')).toHaveTextContent(
      'Over your tailnet (100.122.51.13)'
    );
    expect(screen.queryByText(/closed it without sending a byte/)).toBeNull();
    fireEvent.click(screen.getByTestId('link-route-control-details'));
    expect(screen.getByText(/closed it without sending a byte/)).toBeInTheDocument();
    expect(screen.getByText(/did not answer/)).toBeInTheDocument();
  });

  it('renders nothing before any request', () => {
    const { container } = render(<LinkRoutesPanel routes={{}} />);
    expect(container).toBeEmptyDOMElement();
  });
});
