import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Laptop, RefreshCw } from 'lucide-react';
import {
  controllerCatalog,
  controllerErrorText,
  controllerStatus,
  controllerSync,
  type ControllerCatalog,
  type ControllerStatus,
} from '../../acp/controller';
import { leanzeroLinkStatus } from '../../acp/leanzero-link';
import { workKey, type CatalogEntry, type HostDescriptor } from '../../acp/controllerTypes';
import { defineMessages, useIntl } from '../../i18n';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Checkbox } from '../lz/Checkbox';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import ControllerWorkView from './ControllerWorkView';

const messages = defineMessages({
  title: { id: 'controller.catalog.title', defaultMessage: 'Sessions across your Macs' },
  detail: {
    id: 'controller.catalog.detail',
    defaultMessage: 'Open history, answer requests and guide work on the Mac that owns it.',
  },
  sync: { id: 'controller.catalog.sync', defaultMessage: 'Sync sessions' },
  search: { id: 'controller.catalog.search', defaultMessage: 'Search linked sessions' },
  archived: { id: 'controller.catalog.archived', defaultMessage: 'Include archived sessions' },
  signIn: {
    id: 'controller.catalog.signIn',
    defaultMessage:
      'Sign in to LeanZero Link under Providers → LeanZero MLX → My Macs, then connect to see your Macs here.',
  },
  loading: { id: 'controller.catalog.loading', defaultMessage: 'Connecting to your linked Macs…' },
  empty: {
    id: 'controller.catalog.empty',
    defaultMessage: 'No compatible linked Macs are available.',
  },
  noSessions: {
    id: 'controller.catalog.noSessions',
    defaultMessage: 'No matching sessions on this Mac.',
  },
  stale: { id: 'controller.catalog.stale', defaultMessage: 'Saved state — reconnecting' },
  online: { id: 'controller.catalog.online', defaultMessage: 'Online' },
  offline: { id: 'controller.catalog.offline', defaultMessage: 'Offline' },
  update_required: {
    id: 'controller.catalog.updateRequired',
    defaultMessage: 'Goose update required',
  },
  owner: { id: 'controller.catalog.owner', defaultMessage: 'Owner: {name}' },
  count: {
    id: 'controller.catalog.count',
    defaultMessage: '{count, plural, one {# session} other {# sessions}}',
  },
});

export default function LinkedSessionsView({ navigation }: { navigation: ReactNode }) {
  const intl = useIntl();
  const [status, setStatus] = useState<ControllerStatus | null>(null);
  const [catalog, setCatalog] = useState<ControllerCatalog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [search, setSearch] = useState('');
  const [archived, setArchived] = useState(false);
  const [selected, setSelected] = useState<{ entry: CatalogEntry; host: HostDescriptor } | null>(
    null
  );
  const currentDevice = useRef<string | null>(null);
  const alive = useRef(false);
  const inFlight = useRef(false);
  const refresh = useCallback(async (manual = false) => {
    if (inFlight.current) return;
    inFlight.current = true;
    if (manual) setSyncing(true);
    try {
      const auth = await controllerStatus();
      if (!alive.current) return;
      if (auth.device?.device_id !== currentDevice.current) {
        setCatalog(null);
        setSelected(null);
        currentDevice.current = auth.device?.device_id ?? null;
      }
      if (auth.state === 'signed_out') {
        // Controller enrollment starts after Link connects; absent controller credentials
        // do not establish that the Link account itself is signed out.
        const link = await leanzeroLinkStatus();
        if (!alive.current) return;
        const signedOut = link.auth.state === 'loggedOut' || link.auth.state === 'codeSent';
        setStatus(signedOut ? auth : null);
        setCatalog(null);
        setSelected(null);
        if (!signedOut && auth.error) throw new Error(auth.error);
        setError(null);
        return;
      }
      setStatus(auth);
      if (auth.state !== 'ready')
        throw new Error(auth.error ?? 'The desktop controller is unavailable.');
      const next = await (manual ? controllerSync() : controllerCatalog());
      if (alive.current) {
        setCatalog(next);
        setError(null);
      }
    } catch (failure) {
      if (alive.current) setError(controllerErrorText(failure));
    } finally {
      inFlight.current = false;
      if (alive.current) setSyncing(false);
    }
  }, []);
  useEffect(() => {
    alive.current = true;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const read = async () => {
      await refresh();
      if (alive.current) timer = setTimeout(() => void read(), 2000);
    };
    void read();
    return () => {
      alive.current = false;
      if (timer) clearTimeout(timer);
    };
  }, [refresh]);
  const selectedHost =
    selected &&
    catalog?.hosts.find((row) => row.host.owner_node_id === selected.host.owner_node_id);
  const catalogPending = !catalog || (catalog.updated_at === null && catalog.hosts.length === 0);
  return (
    <MainPanelLayout>
      <div className="flex-1 flex flex-col min-h-0">
        <header className="px-6 pt-12 pb-4 space-y-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <h1 className="text-3xl font-light">{intl.formatMessage(messages.title)}</h1>
            <Button variant="outline" disabled={syncing} onClick={() => void refresh(true)}>
              <RefreshCw className={`w-4 h-4 mr-2 ${syncing ? 'animate-spin' : ''}`} />
              {intl.formatMessage(messages.sync)}
            </Button>
          </div>
          <p className="text-sm text-text-secondary">{intl.formatMessage(messages.detail)}</p>
          {navigation}
        </header>
        {error && (
          <p role="alert" className="px-6 py-2 text-sm text-text-danger">
            {error}
          </p>
        )}
        {selected ? (
          <div className="flex-1 min-h-0">
            <ControllerWorkView
              key={`${workKey(selected.entry.work)}:${selected.entry.store_generation}`}
              entry={selected.entry}
              host={selectedHost?.host ?? selected.host}
              stale={
                Boolean(error) ||
                !selectedHost ||
                selectedHost.stale ||
                !selectedHost.entries.some(
                  (entry) => workKey(entry.work) === workKey(selected.entry.work)
                )
              }
              onBack={() => setSelected(null)}
            />
          </div>
        ) : (
          <div className="overflow-auto px-6 pb-6 space-y-5">
            <Input
              aria-label={intl.formatMessage(messages.search)}
              placeholder={intl.formatMessage(messages.search)}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
            <Checkbox
              checked={archived}
              onChange={setArchived}
              label={intl.formatMessage(messages.archived)}
            />
            {status?.state === 'signed_out' && <p>{intl.formatMessage(messages.signIn)}</p>}
            {catalogPending && !error && (!status || status.state === 'ready') && (
              <p>{intl.formatMessage(messages.loading)}</p>
            )}
            {!catalogPending && catalog?.hosts.length === 0 && (
              <p>{intl.formatMessage(messages.empty)}</p>
            )}
            {catalog?.hosts.map((row) => {
              const entries = row.entries
                .filter(
                  (entry) =>
                    (archived || !entry.archived) &&
                    `${entry.title} ${entry.working_dir ?? ''} ${row.host.name}`
                      .toLocaleLowerCase()
                      .includes(search.toLocaleLowerCase())
                )
                .sort((a, b) => b.updated_at.localeCompare(a.updated_at));
              return (
                <section key={row.host.owner_node_id} className="space-y-3">
                  <div className="flex flex-wrap gap-3 items-center">
                    <Laptop className="w-5 h-5" />
                    <h2 className="font-semibold">{row.host.name}</h2>
                    <span className="text-sm text-text-secondary">
                      {intl.formatMessage(
                        error || row.stale ? messages.stale : messages[row.host.availability]
                      )}
                    </span>
                    <span className="text-sm text-text-secondary">
                      {intl.formatMessage(messages.count, { count: entries.length })}
                    </span>
                  </div>
                  {row.error && (
                    <p role="status" className="text-sm text-text-danger">
                      {row.error}
                    </p>
                  )}
                  {!entries.length && (
                    <p className="text-sm text-text-secondary">
                      {intl.formatMessage(messages.noSessions)}
                    </p>
                  )}
                  <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
                    {entries.map((entry) => (
                      <button
                        key={workKey(entry.work)}
                        onClick={() => setSelected({ entry, host: row.host })}
                        className="text-left p-4 rounded-xl border border-border-primary bg-background-secondary space-y-2 focus-visible:ring-2 focus-visible:ring-lz-accent"
                      >
                        <h3 className="font-semibold break-words">{entry.title}</h3>
                        <p className="text-sm">
                          {intl.formatMessage(messages.owner, { name: row.host.name })}
                        </p>
                        <p className="text-xs text-text-secondary">
                          {entry.work.kind} · {entry.state}
                        </p>
                        {entry.last_event && (
                          <p className="text-sm line-clamp-2 break-words">
                            {entry.last_event.text}
                          </p>
                        )}
                        {entry.working_dir && (
                          <p className="text-xs text-text-secondary truncate">
                            {entry.working_dir}
                          </p>
                        )}
                      </button>
                    ))}
                  </div>
                </section>
              );
            })}
          </div>
        )}
      </div>
    </MainPanelLayout>
  );
}
