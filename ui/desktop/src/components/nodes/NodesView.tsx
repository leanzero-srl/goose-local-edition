import React, { useCallback, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { ScrollArea } from '../ui/scroll-area';
import SwarmNodesSection from '../leanzero-swarm/SwarmNodesSection';
import { Disclosure, PageHeader, SURFACE, Segmented, TYPE, cx, type SegmentedOption } from '../lz';
import { defineMessages, useIntl } from '../../i18n';
import { cloudHref, nodesTabOf, type NodesTab as NodesTabName } from '../../utils/navigationUtils';
import { useGlanceNodes } from '../engineGlance/glanceStore';
import { NodesTab } from './NodesTab';
import { StrategiesTab } from './StrategiesTab';
import { UseSelectors } from './UseSelectors';
import { useBuildEligibility } from './useBuildEligibility';

const i18n = defineMessages({
  title: { id: 'nodes.title', defaultMessage: 'Nodes' },
  subtitle: {
    id: 'nodes.subtitle',
    defaultMessage:
      'A node is a model you can hand work to, on your Macs or in the cloud. Chats start on a node or a strategy; swarm builds use a strategy or your swarm pool.',
  },
  sections: { id: 'nodes.sections', defaultMessage: 'Nodes sections' },
  tabNodes: { id: 'nodes.tabNodes', defaultMessage: 'Nodes' },
  tabStrategies: { id: 'nodes.tabStrategies', defaultMessage: 'Strategies' },
  poolSection: {
    id: 'nodes.poolSection',
    defaultMessage: 'Your swarm pool · swarm builds and chats on Any node (Auto) run on these',
  },
  poolSectionStrategy: {
    id: 'nodes.poolSectionStrategy',
    defaultMessage:
      'Your swarm pool · chats on Any node (Auto) run on these; swarm builds use {strategy} now',
  },
});

/**
 * Nodes — everything a person hands work to, first in the left nav (Q-193; DESIGN-NODES-AND-
 * STRATEGIES.md §5, §8.2, §8.4). Two tabs in the URL (`?tab=nodes|strategies`, written with
 * `replace` so Back returns to the tab that was open); `&node=<id>` outlines and scrolls to that
 * card (NodesTab), `&strategy=<id>` opens that strategy's editor (StrategiesTab).
 *
 * The page shell: the two uses ("New chats start on", "Swarm builds use" — UseSelectors, written
 * through `nodes/write`), the node cards, and YOUR SWARM POOL — `SwarmNodesSection` unchanged, the one
 * UI writer of the `swarm` block (review item 7): shown open while swarm builds use the pool, folded
 * into a Disclosure while they use a strategy. A card's "Edit in your swarm pool" opens and scrolls
 * to it; nothing on this page writes `swarm` any other way.
 */
const NodesView: React.FC = () => {
  const intl = useIntl();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const tab = nodesTabOf(searchParams.get('tab'));
  const store = useGlanceNodes();
  const read = store.kind === 'read' ? store.read : null;
  const config = read?.config ?? null;
  const eligibility = useBuildEligibility(config);

  const forBuilds = config?.forBuilds ?? { kind: 'pool' as const };
  const buildsUsePool = forBuilds.kind === 'pool';
  // What the pool is for, said from the two selectors' real values — never the selector restated
  // (Q-317: 'used by swarm builds while “Swarm builds use” is “Your swarm pool”').
  const poolTitle =
    forBuilds.kind === 'strategy'
      ? intl.formatMessage(i18n.poolSectionStrategy, {
          strategy:
            (config?.strategies ?? []).find((s) => s.id === forBuilds.id)?.name ?? forBuilds.id,
        })
      : intl.formatMessage(i18n.poolSection);
  const [poolOpen, setPoolOpen] = useState(false);
  const poolRef = useRef<HTMLDivElement>(null);

  const selectTab = useCallback(
    (next: NodesTabName) => setSearchParams({ tab: next }, { replace: true }),
    [setSearchParams]
  );

  const editInPool = useCallback(() => {
    setPoolOpen(true);
    // The pool renders open on the next paint; scroll once it is there.
    requestAnimationFrame(() => poolRef.current?.scrollIntoView?.({ block: 'start' }));
  }, []);

  const tabs: SegmentedOption<NodesTabName>[] = [
    { value: 'nodes', label: intl.formatMessage(i18n.tabNodes) },
    { value: 'strategies', label: intl.formatMessage(i18n.tabStrategies) },
  ];

  // The pool's table is a fixed four-column grid (unchanged, its own component): at the narrow window
  // it scrolls sideways inside its own box instead of being clipped by its card (measured at 460:
  // the table needs ~790px and its card cut Provider/Model/Share off).
  const pool = (
    <div className="min-w-0 overflow-x-auto" data-testid="nodes-pool-scroll">
      <div className="min-w-[50rem]">
        <SwarmNodesSection onOpenCloudProviders={() => navigate(cloudHref())} />
      </div>
    </div>
  );

  return (
    <MainPanelLayout>
      <div className={cx('flex min-h-0 flex-1 flex-col', SURFACE.page)}>
        <div className={cx('border-b px-lz-page pb-6 pt-16', SURFACE.hairline)}>
          <PageHeader
            className="page-transition"
            title={intl.formatMessage(i18n.title)}
            subtitle={
              <span className="block max-w-[70ch]">{intl.formatMessage(i18n.subtitle)}</span>
            }
            actions={
              <Segmented
                aria-label={intl.formatMessage(i18n.sections)}
                options={tabs}
                value={tab}
                onChange={selectTab}
              />
            }
          />
        </div>

        <div className="relative min-h-0 flex-1 px-lz-page pt-6">
          <ScrollArea className="h-full">
            <div className="flex flex-col gap-6 pb-lz-page">
              {config && read && (
                <UseSelectors config={config} nodes={read.nodes} eligibility={eligibility} />
              )}

              {tab === 'nodes' && (
                <div className="flex flex-col gap-6" data-testid="nodes-tab">
                  <NodesTab onEditInPool={editInPool} />
                  <div ref={poolRef} data-testid="nodes-pool" data-open={buildsUsePool || poolOpen}>
                    {buildsUsePool ? (
                      <section className="flex flex-col gap-3">
                        <span className={TYPE.zone} data-testid="nodes-pool-title">
                          {poolTitle}
                        </span>
                        {pool}
                      </section>
                    ) : (
                      <Disclosure
                        title={poolTitle}
                        open={poolOpen}
                        onOpenChange={setPoolOpen}
                        testId="nodes-pool-disclosure"
                      >
                        <div className="p-4">{pool}</div>
                      </Disclosure>
                    )}
                  </div>
                </div>
              )}
              {tab === 'strategies' && (
                <div data-testid="strategies-tab">
                  <StrategiesTab eligibility={eligibility} />
                </div>
              )}
            </div>
          </ScrollArea>
        </div>
      </div>
    </MainPanelLayout>
  );
};

export default NodesView;
