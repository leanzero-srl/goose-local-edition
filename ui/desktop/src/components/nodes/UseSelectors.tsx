import { useState } from 'react';
import type { BuildEligibility } from '../../acp/nodes';
import { nodesWrite } from '../../acp/nodes';
import { defineMessages, useIntl } from '../../i18n';
import { TYPE, cx } from '../lz';
import { StudioSelect, ToneBanner, type StudioSelectOption } from '../leanzero-swarm/studio';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { refreshGlanceNodes } from '../engineGlance/glanceStore';
import {
  namedStrategies,
  nodeNamesById,
  type NodesConfig,
  type NodesForBuilds,
  type NodesForNewChats,
  type ResolvedNodeDef,
} from './model';
import type { Read } from './nodeGlance';
import { buildReasonWords } from './StrategyCard';

/**
 * "New chats start on" and "Swarm builds use" (DESIGN-NODES-AND-STRATEGIES.md §8.2) — the page's
 * two uses, each written through the ONE door, `nodes/write` (a change of `forNewChats` also sets
 * the global defaults in that same call, §4.2). A refusal is shown verbatim and nothing moves; a
 * strategy swarm builds cannot use stays VISIBLE in the list with its reason and cannot be picked.
 */

const i18n = defineMessages({
  forNewChats: { id: 'nodes.forNewChats', defaultMessage: 'New chats start on:' },
  forBuilds: { id: 'nodes.forBuilds', defaultMessage: 'Swarm builds use:' },
  auto: { id: 'nodes.auto', defaultMessage: 'Any node (Auto)' },
  pool: { id: 'nodes.pool', defaultMessage: 'Your swarm pool' },
  optionStrategy: { id: 'nodes.optionStrategy', defaultMessage: '{name} (strategy)' },
  optionNode: { id: 'nodes.optionNode', defaultMessage: '{name} (node)' },
  missing: { id: 'nodes.useMissing', defaultMessage: '{id} (no longer defined)' },
  checking: { id: 'nodes.buildsChecking', defaultMessage: 'checking…' },
  notSaved: { id: 'nodes.useNotSaved', defaultMessage: 'Not changed' },
  failed: { id: 'nodes.useFailed', defaultMessage: 'The change could not be saved' },
});

interface UseOption extends StudioSelectOption {
  hint?: string;
}

export function newChatsValue(forNewChats: NodesForNewChats | undefined): string {
  const use = forNewChats ?? { kind: 'auto' };
  return use.kind === 'auto' ? 'auto' : `${use.kind}:${use.id}`;
}

export function buildsValue(forBuilds: NodesForBuilds | undefined): string {
  const use = forBuilds ?? { kind: 'pool' };
  return use.kind === 'pool' ? 'pool' : `strategy:${use.id}`;
}

function newChatsOf(value: string): NodesForNewChats {
  if (value === 'auto') return { kind: 'auto' };
  const at = value.indexOf(':');
  return { kind: value.slice(0, at) as 'node' | 'strategy', id: value.slice(at + 1) };
}

function buildsOf(value: string): NodesForBuilds {
  return value === 'pool'
    ? { kind: 'pool' }
    : { kind: 'strategy', id: value.slice('strategy:'.length) };
}

export interface UseSelectorsProps {
  config: NodesConfig;
  nodes: readonly ResolvedNodeDef[];
  eligibility: Record<string, Read<BuildEligibility>>;
}

export function UseSelectors({ config, nodes, eligibility }: UseSelectorsProps) {
  const intl = useIntl();
  const [busy, setBusy] = useState(false);
  const [refusals, setRefusals] = useState<string[]>([]);
  // A chat's own node set is that chat's alone: never what new chats start on or builds use.
  const strategies = namedStrategies(config);
  const names = nodeNamesById(nodes);

  const chatOptions: UseOption[] = [
    { value: 'auto', label: intl.formatMessage(i18n.auto) },
    ...strategies.map((s) => ({
      value: `strategy:${s.id}`,
      label: intl.formatMessage(i18n.optionStrategy, { name: s.name }),
    })),
    ...nodes.map((n) => ({
      value: `node:${n.def.id}`,
      label: intl.formatMessage(i18n.optionNode, { name: n.def.name }),
    })),
  ];
  const buildOptions: UseOption[] = [
    { value: 'pool', label: intl.formatMessage(i18n.pool) },
    ...strategies.map((s) => {
      const answer = eligibility[s.id];
      const label = intl.formatMessage(i18n.optionStrategy, { name: s.name });
      if (answer?.kind === 'read' && answer.value.eligible) {
        return { value: `strategy:${s.id}`, label };
      }
      const hint =
        answer?.kind === 'read'
          ? (answer.value.reasons ?? []).map((r) => buildReasonWords(intl, r, names, [])).join('; ')
          : answer?.kind === 'failed'
            ? answer.error
            : intl.formatMessage(i18n.checking);
      return { value: `strategy:${s.id}`, label, hint, disabled: true };
    }),
  ];

  // The stored choice is always shown, even one whose target is gone (never silently "Auto").
  const withCurrent = (options: UseOption[], value: string): UseOption[] =>
    options.some((o) => o.value === value)
      ? options
      : [
          ...options,
          {
            value,
            label: intl.formatMessage(i18n.missing, { id: value.slice(value.indexOf(':') + 1) }),
            disabled: true,
          },
        ];
  const chatValue = newChatsValue(config.forNewChats);
  const buildValue = buildsValue(config.forBuilds);
  const chatList = withCurrent(chatOptions, chatValue);
  const buildList = withCurrent(buildOptions, buildValue);

  const write = async (next: NodesConfig) => {
    setBusy(true);
    setRefusals([]);
    try {
      const response = await nodesWrite(next);
      if (!response.written) setRefusals((response.refusals ?? []).map((r) => r.message));
    } catch (e) {
      setRefusals([mlxErrorMessage(e, intl.formatMessage(i18n.failed))]);
    } finally {
      setBusy(false);
      refreshGlanceNodes();
    }
  };

  const render = (o: UseOption, where: 'option' | 'value') => (
    <span className="flex min-w-0 items-baseline gap-2" title={o.hint}>
      <span className="shrink-0 truncate">{o.label}</span>
      {where === 'option' && o.hint && (
        <span className={cx('min-w-0 truncate', TYPE.meta)}>{o.hint}</span>
      )}
    </span>
  );

  return (
    <div className="flex flex-col gap-2" data-testid="use-selectors">
      <div className="grid grid-cols-1 gap-3 min-[720px]:grid-cols-2">
        <div className="flex min-w-0 flex-col gap-1">
          <span className={cx(TYPE.meta)}>{intl.formatMessage(i18n.forNewChats)}</span>
          <StudioSelect<UseOption>
            aria-label={intl.formatMessage(i18n.forNewChats)}
            options={chatList}
            value={chatList.find((o) => o.value === chatValue) ?? null}
            placeholder={intl.formatMessage(i18n.auto)}
            loading={busy}
            renderOption={render}
            optionTestId={(o) => `use-chats-${o.value}`}
            onChange={(o) => {
              if (o && o.value !== chatValue)
                void write({ ...config, forNewChats: newChatsOf(o.value) });
            }}
          />
        </div>
        <div className="flex min-w-0 flex-col gap-1">
          <span className={cx(TYPE.meta)}>{intl.formatMessage(i18n.forBuilds)}</span>
          <StudioSelect<UseOption>
            aria-label={intl.formatMessage(i18n.forBuilds)}
            options={buildList}
            value={buildList.find((o) => o.value === buildValue) ?? null}
            placeholder={intl.formatMessage(i18n.pool)}
            loading={busy}
            renderOption={render}
            optionTestId={(o) => `use-builds-${o.value}`}
            onChange={(o) => {
              if (o && o.value !== buildValue)
                void write({ ...config, forBuilds: buildsOf(o.value) });
            }}
          />
        </div>
      </div>
      {refusals.length > 0 && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.notSaved)}
          text={refusals.join(' · ')}
          testId="use-refused"
        />
      )}
    </div>
  );
}
