import { useEffect, useMemo, useRef, useState } from 'react';
import { Check, Gauge, Loader2, X } from 'lucide-react';
import {
  acpListOpenRouterHosts,
  acpProbeOpenRouterHost,
  acpReadOpenRouterPin,
  acpSetOpenRouterPin,
  type OpenRouterHost,
  type OpenRouterPin,
} from '../../acp/openrouter';
import { Button, Chip, DataTable, TONE_TEXT, TYPE, cx, type DataTableColumn } from '../lz';
import { StudioSelect, type StudioSelectOption } from '../leanzero-swarm/studio';
import {
  ANY_HOST,
  formatSeconds,
  formatSpeed,
  hostLabel,
  pinSummary,
  probeError,
  rankProbes,
  recommendedTag,
  splitByTools,
  type ProbeRow,
  type ProbeState,
} from './hostProbe';

/** How long the model id must stay unchanged before its hosts are fetched — a typed id is not
 *  listed once per keystroke. */
const LIST_AFTER_TYPING_MS = 500;

interface Listing {
  model: string;
  hosts: OpenRouterHost[];
  untagged: string[];
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/**
 * Which OpenRouter host runs `model`, and a parallel speed + tool-call check of every host that
 * lists tool support. The choice is OPENROUTER_PARAMETERS in goose's config — the key the
 * OpenRouter provider sends with every request and the benchmark driver writes before each run —
 * so this picker, chats and benchmark runs always agree on the host.
 */
export function OpenRouterHostPicker({
  model,
  disabled = false,
}: {
  model: string;
  disabled?: boolean;
}) {
  const id = model.trim();
  const [pin, setPin] = useState<OpenRouterPin | null>(null);
  const [pinError, setPinError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [listing, setListing] = useState<Listing | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [loadingHosts, setLoadingHosts] = useState(false);
  const [probes, setProbes] = useState<Record<string, ProbeState>>({});
  const generation = useRef(0);

  // Re-read on focus too: the benchmark driver rewrites the same key between runs, and the picker
  // must never show a host the file no longer names.
  useEffect(() => {
    let alive = true;
    const read = () =>
      acpReadOpenRouterPin()
        .then((current) => {
          if (!alive) return;
          setPin(current);
          setPinError(null);
        })
        .catch((err: unknown) => {
          if (alive) setPinError(`The saved host could not be read: ${message(err)}`);
        });
    void read();
    window.addEventListener('focus', read);
    return () => {
      alive = false;
      window.removeEventListener('focus', read);
    };
  }, [id]);

  useEffect(() => {
    generation.current += 1;
    setListing(null);
    setListError(null);
    setProbes({});
    if (!id) {
      setLoadingHosts(false);
      return undefined;
    }
    let alive = true;
    setLoadingHosts(true);
    const timer = setTimeout(() => {
      acpListOpenRouterHosts(id)
        .then((answer) => {
          if (alive)
            setListing({ model: id, hosts: answer.hosts, untagged: answer.untagged ?? [] });
        })
        .catch((err: unknown) => {
          if (alive)
            setListError(`OpenRouter's hosts for ${id} could not be listed: ${message(err)}`);
        })
        .finally(() => {
          if (alive) setLoadingHosts(false);
        });
    }, LIST_AFTER_TYPING_MS);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [id]);

  const { tools, noTools } = useMemo(() => splitByTools(listing?.hosts ?? []), [listing]);

  const choose = (tag: string | null) => {
    setSaving(true);
    setPinError(null);
    acpSetOpenRouterPin(tag)
      .then(setPin)
      .catch((err: unknown) => setPinError(`The host could not be saved: ${message(err)}`))
      .finally(() => setSaving(false));
  };

  const testHosts = () => {
    if (!listing) return;
    const run = ++generation.current;
    setProbes(Object.fromEntries(tools.map((host) => [host.tag, { kind: 'running' } as const])));
    for (const host of tools) {
      acpProbeOpenRouterHost(listing.model, host.tag)
        .then((result) => {
          if (run === generation.current)
            setProbes((all) => ({ ...all, [host.tag]: { kind: 'done', result } }));
        })
        .catch((err: unknown) => {
          if (run === generation.current)
            setProbes((all) => ({ ...all, [host.tag]: { kind: 'failed', error: message(err) } }));
        });
    }
  };

  const rows: ProbeRow[] = useMemo(
    () =>
      rankProbes(
        tools
          .filter((host) => probes[host.tag] != null)
          .map((host) => ({ host, state: probes[host.tag] }))
      ),
    [tools, probes]
  );
  const running = rows.filter((row) => row.state.kind === 'running').length;
  const recommended = running === 0 ? recommendedTag(rows) : null;

  const options: StudioSelectOption[] = useMemo(() => {
    const list: StudioSelectOption[] = [
      { value: ANY_HOST, label: 'Any host (OpenRouter routes)' },
      ...tools.map((host) => ({ value: host.tag, label: hostLabel(host) })),
    ];
    if (pin?.tag && !tools.some((host) => host.tag === pin.tag)) {
      list.push({
        value: pin.tag,
        label: listing
          ? `${pin.tag} (saved pin — not a tool host for this model)`
          : `${pin.tag} (saved pin)`,
      });
    }
    if (pin?.raw != null && pin.tag == null) {
      list.push({
        value: pin.raw,
        label: 'Custom routing (OPENROUTER_PARAMETERS)',
        disabled: true,
      });
    }
    return list;
  }, [tools, pin, listing]);
  const selected =
    pin == null
      ? null
      : (options.find(
          (option) => option.value === (pin.raw == null ? ANY_HOST : (pin.tag ?? pin.raw))
        ) ?? null);
  const summary = pin ? pinSummary(pin, listing?.hosts ?? null, id) : null;

  const columns: DataTableColumn<ProbeRow>[] = [
    {
      key: 'host',
      header: 'Host',
      cell: (row) => (
        <span className="flex flex-wrap items-center gap-2">
          <span>{row.host.providerName}</span>
          <span className={TYPE.meta}>{row.host.tag}</span>
          {row.host.tag === recommended && <Chip tone="ok">Recommended</Chip>}
          {row.host.tag === pin?.tag && <Chip tone="accent">Pinned</Chip>}
        </span>
      ),
    },
    {
      key: 'speed',
      header: 'tok/s',
      numeric: true,
      cell: (row) =>
        row.state.kind === 'running' ? (
          <span className="inline-flex items-center gap-1" aria-label="Testing">
            <Loader2 className="size-3.5 animate-spin" />
            testing
          </span>
        ) : row.state.kind === 'done' ? (
          (formatSpeed(row.state.result) ?? '—')
        ) : (
          '—'
        ),
    },
    {
      key: 'tool',
      header: 'Tool call',
      cell: (row) =>
        row.state.kind !== 'done' || row.state.result.error ? (
          '—'
        ) : row.state.result.toolCall ? (
          <span className={cx('inline-flex items-center gap-1', TONE_TEXT.ok)}>
            <Check className="size-4" aria-hidden />
            yes
          </span>
        ) : (
          <span className={cx('inline-flex items-center gap-1', TONE_TEXT.err)}>
            <X className="size-4" aria-hidden />
            no
          </span>
        ),
    },
    {
      key: 'seconds',
      header: 'Seconds',
      numeric: true,
      cell: (row) => (row.state.kind === 'done' ? formatSeconds(row.state.result.seconds) : '—'),
    },
    {
      key: 'finish',
      header: 'Finish',
      className: 'whitespace-nowrap',
      cell: ({ state }) => {
        if (state.kind === 'running') return null;
        if (state.kind === 'failed') return <span className={TONE_TEXT.err}>failed</span>;
        if (state.result.error)
          return (
            <span className={TONE_TEXT.err}>
              {state.result.httpStatus != null ? `HTTP ${state.result.httpStatus}` : 'failed'}
            </span>
          );
        return state.result.finishReason ?? '—';
      },
    },
  ];

  if (!id) return null;

  return (
    <div className="flex flex-col gap-2 text-sm" data-testid="openrouter-host-picker">
      <div className="flex flex-wrap items-center gap-3">
        <span className={TYPE.meta}>Host</span>
        <StudioSelect
          aria-label="OpenRouter host"
          className="w-full max-w-[440px]"
          options={options}
          value={selected}
          placeholder="Reading the saved host…"
          loading={pin == null && pinError == null}
          disabled={disabled || saving || pin == null}
          onChange={(option) => {
            if (!option || option.disabled) return;
            choose(option.value === ANY_HOST ? null : option.value);
          }}
          optionTestId={(option) => `openrouter-host-${option.value}`}
        />
        <Button
          variant="secondary"
          icon={running > 0 ? <Loader2 className="animate-spin" /> : <Gauge />}
          disabled={disabled || !listing || tools.length === 0 || running > 0}
          onClick={testHosts}
          title="Send one short call with a ping tool to every host below, all at once, and measure each"
        >
          {running > 0
            ? `Testing… ${rows.length - running} of ${rows.length} answered`
            : 'Test hosts'}
        </Button>
      </div>
      {summary && (
        <p className={summary.tone === 'err' ? 'text-lz-meta text-lz-err' : TYPE.meta}>
          {summary.text}
        </p>
      )}
      {pinError && (
        <p role="alert" className="text-lz-err">
          {pinError}
        </p>
      )}
      {loadingHosts && <p className={TYPE.meta}>Reading OpenRouter's hosts for {id}…</p>}
      {listError && (
        <p role="alert" className="text-lz-err">
          {listError}
        </p>
      )}
      {listing && tools.length === 0 && (
        <p className="text-lz-meta text-lz-err">
          No host lists tool support for {listing.model}; a goose run needs tool calls.
        </p>
      )}
      {listing && (noTools.length > 0 || listing.untagged.length > 0) && (
        <p className={TYPE.meta}>
          {[
            noTools.length > 0 &&
              `Not offered, no tool support: ${noTools.map((h) => `${h.providerName} (${h.tag})`).join(', ')}.`,
            listing.untagged.length > 0 &&
              `Not offered, OpenRouter gives no tag to pin: ${listing.untagged.join(', ')}.`,
          ]
            .filter(Boolean)
            .join(' ')}
        </p>
      )}
      {rows.length > 0 && (
        <section aria-label="Host tests" className="flex flex-col gap-1">
          <span className={TYPE.meta}>
            {rows.length} {rows.length === 1 ? 'host' : 'hosts'} tested on {listing?.model} — one
            call each with a ping tool, all at once, billed by OpenRouter.
            {running === 0 && !recommended && ' No host answered with the tool call.'}
          </span>
          <DataTable
            aria-label="Host test results"
            dense
            columns={columns}
            rows={rows}
            rowKey={(row) => row.host.tag}
            rowTestId={(row) => `openrouter-probe-${row.host.tag}`}
            renderSubRow={(row) => {
              const error = probeError(row.state);
              return error ? (
                <span className="block whitespace-pre-wrap break-words text-lz-err">{error}</span>
              ) : null;
            }}
            rowAction={(row) => (
              <Button
                size="sm"
                variant={row.host.tag === recommended ? 'primary' : 'secondary'}
                disabled={disabled || saving || row.host.tag === pin?.tag}
                onClick={() => choose(row.host.tag)}
              >
                {row.host.tag === pin?.tag ? 'In use' : 'Use'}
              </Button>
            )}
          />
        </section>
      )}
    </div>
  );
}
