import React, { useCallback } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { Cloud, Laptop, Route as RouteIcon } from 'lucide-react';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { ScrollArea } from '../ui/scroll-area';
import SwarmNodesSection from '../leanzero-swarm/SwarmNodesSection';
import { Button, EmptyState, PageHeader, SURFACE, Segmented, cx, type SegmentedOption } from '../lz';
import { defineMessages, useIntl } from '../../i18n';
import {
  cloudHref,
  mlxHref,
  nodesTabOf,
  type NodesTab,
} from '../../utils/navigationUtils';

const i18n = defineMessages({
  title: { id: 'nodes.title', defaultMessage: 'Nodes' },
  subtitle: {
    id: 'nodes.subtitle',
    defaultMessage:
      'A node is a model you can hand work to, on your Macs or in the cloud. Chats start on a node or a strategy; swarm builds use a strategy.',
  },
  sections: { id: 'nodes.sections', defaultMessage: 'Nodes sections' },
  tabNodes: { id: 'nodes.tabNodes', defaultMessage: 'Nodes' },
  tabStrategies: { id: 'nodes.tabStrategies', defaultMessage: 'Strategies' },
  manageMacs: { id: 'nodes.manageMacs', defaultMessage: 'Manage Macs and models' },
  manageCloud: { id: 'nodes.manageCloud', defaultMessage: 'Manage cloud providers' },
  strategiesSoonTitle: {
    id: 'nodes.strategiesSoonTitle',
    defaultMessage: 'Strategies are coming in this release',
  },
  strategiesSoonBody: {
    id: 'nodes.strategiesSoonBody',
    defaultMessage:
      'A strategy will say which node does what: its roles, the order to try nodes in, and when to use the next one. Until then, chats and swarm builds use the nodes on the Nodes tab.',
  },
});

/**
 * Nodes — everything a person hands work to, first in the left nav (Q-193; DESIGN-NODES-AND-
 * STRATEGIES.md §5). Two tabs, in the URL (`?tab=nodes|strategies`, written with `replace` so Back
 * returns to the tab that was open).
 *
 * This view is deliberately a THIN host: the Nodes tab renders the swarm pool table exactly as it
 * was under Providers › Swarm Settings, and the node cards that replace it are S2's. The Strategies
 * tab says honestly that strategies are not here yet — S6 fills it.
 */
const NodesView: React.FC = () => {
  const intl = useIntl();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const tab = nodesTabOf(searchParams.get('tab'));

  const selectTab = useCallback(
    (next: NodesTab) => setSearchParams({ tab: next }, { replace: true }),
    [setSearchParams]
  );

  const tabs: SegmentedOption<NodesTab>[] = [
    { value: 'nodes', label: intl.formatMessage(i18n.tabNodes) },
    { value: 'strategies', label: intl.formatMessage(i18n.tabStrategies) },
  ];

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
            {tab === 'nodes' && (
              <div className="flex flex-col gap-4 pb-lz-page" data-testid="nodes-tab">
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={<Laptop />}
                    onClick={() => navigate(mlxHref('macs'))}
                  >
                    {intl.formatMessage(i18n.manageMacs)}
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={<Cloud />}
                    onClick={() => navigate(cloudHref())}
                  >
                    {intl.formatMessage(i18n.manageCloud)}
                  </Button>
                </div>
                <SwarmNodesSection onOpenCloudProviders={() => navigate(cloudHref())} />
              </div>
            )}
            {tab === 'strategies' && (
              <div data-testid="strategies-tab">
                <EmptyState
                  icon={<RouteIcon />}
                  title={intl.formatMessage(i18n.strategiesSoonTitle)}
                  body={intl.formatMessage(i18n.strategiesSoonBody)}
                />
              </div>
            )}
          </ScrollArea>
        </div>
      </div>
    </MainPanelLayout>
  );
};

export default NodesView;
