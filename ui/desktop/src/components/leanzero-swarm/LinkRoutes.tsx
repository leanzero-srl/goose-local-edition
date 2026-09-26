import { useState } from 'react';
import { Button, Panel, StatusDot, TONE_TEXT, TYPE, WEIGHT, cx, type Tone } from '../lz';
import type { LinkRoute, LinkRoutes } from '../../acp/leanzero-link';

/**
 * How Link last reached its two servers (Q-137). A server with a Tailscale `*.ts.net` name has two
 * doors: its tailnet address, used when this Mac is on that node's tailnet, and its public Tailscale
 * Funnel address. goosed picks per request and reports the pick; this panel shows it, so a Funnel
 * outage or a tailnet pin is never invisible behind "not connected".
 */

const SERVER_LABEL: Record<keyof LinkRoutes, string> = {
  worker: 'Sign-in server',
  control: 'Mesh control server',
};

/** The machine label of a tailnet name (`worksmacstudio` of `worksmacstudio.tail….ts.net`). */
function machineOf(host: string): string {
  return host.split('.')[0] || host;
}

function isTailnetName(host: string): boolean {
  return host.toLowerCase().replace(/\.$/, '').endsWith('.ts.net');
}

export function routeLine(route: LinkRoute): { tone: Tone; text: string } {
  if (route.path === 'tailnet') {
    return route.lastFailure
      ? { tone: 'err', text: `Over your tailnet (${route.ip}) — the last request failed` }
      : { tone: 'ok', text: `Over your tailnet (${route.ip})` };
  }
  const funnel = isTailnetName(route.host);
  if (route.lastFailure) {
    return funnel
      ? {
          tone: 'err',
          text: `The public Tailscale address of ${machineOf(route.host)} stopped answering — restart Tailscale on ${machineOf(route.host)}`,
        }
      : { tone: 'err', text: 'Public address — the last request failed' };
  }
  return funnel
    ? { tone: 'warn', text: 'Public Tailscale Funnel address' }
    : { tone: 'secondary', text: 'Public address' };
}

function RouteRow({ server, route }: { server: keyof LinkRoutes; route: LinkRoute }) {
  const [open, setOpen] = useState(false);
  const line = routeLine(route);
  return (
    <div className="flex flex-col gap-1.5" data-testid={`link-route-${server}`}>
      <div className="flex items-center justify-between gap-3">
        <span className={TYPE.meta}>{SERVER_LABEL[server]}</span>
        <Button
          variant="ghost"
          size="sm"
          type="button"
          aria-expanded={open}
          data-testid={`link-route-${server}-details`}
          onClick={() => setOpen((v) => !v)}
        >
          {open ? 'Hide details' : 'Details'}
        </Button>
      </div>
      <span className="inline-flex items-start gap-1.5" data-tone={line.tone}>
        <StatusDot tone={line.tone} label={line.text} />
        <span className={cx(TYPE.body, WEIGHT.semibold, TONE_TEXT[line.tone])}>{line.text}</span>
      </span>
      {open && (
        <dl className={cx('flex flex-col gap-1 break-words', TYPE.meta)}>
          <div>
            <dt className="inline">Server: </dt>
            <dd className="inline">{route.host}</dd>
          </div>
          {route.reason && (
            <div>
              <dt className="inline">Why not the tailnet: </dt>
              <dd className="inline">{route.reason}</dd>
            </div>
          )}
          {route.lastFailure && (
            <div>
              <dt className="inline">Last failure: </dt>
              <dd className="inline">{route.lastFailure}</dd>
            </div>
          )}
          <div>
            <dt className="inline">Decided: </dt>
            <dd className="inline">{new Date(route.decidedAt).toLocaleString()}</dd>
          </div>
        </dl>
      )}
    </div>
  );
}

export function LinkRoutesPanel({ routes }: { routes: LinkRoutes }) {
  const rows = (['worker', 'control'] as const).flatMap((server) => {
    const route = routes[server];
    return route ? [{ server, route }] : [];
  });
  if (rows.length === 0) return null;
  return (
    <div className="mx-auto w-full max-w-md" data-testid="link-routes">
      <Panel title="How Link reaches its servers">
        <div className="flex flex-col gap-4">
          {rows.map(({ server, route }) => (
            <RouteRow key={server} server={server} route={route} />
          ))}
        </div>
      </Panel>
    </div>
  );
}
