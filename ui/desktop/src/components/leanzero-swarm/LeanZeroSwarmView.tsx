import React, { useCallback, useEffect, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { ScrollArea } from '../ui/scroll-area';
import MlxEngineView from './MlxEngineView';
import CloudProvidersSection from './CloudProvidersSection';
import { PageHeader, SURFACE, Segmented, cx, type SegmentedOption } from '../lz';
import { defineMessages, useIntl } from '../../i18n';
import { MacsProvider } from './useMacs';
import { useConfig } from '../ConfigContext';
import type { SwarmConfig } from '../settings/swarm/golden';
import {
  NODES_ROUTE,
  mlxTabOf,
  providersTabOf,
  type MlxTab,
  type ProvidersTab,
} from '../../utils/navigationUtils';

const i18n = defineMessages({
  title: { id: 'providers.title', defaultMessage: 'Providers' },
  subtitle: {
    id: 'providers.subtitle',
    defaultMessage:
      "Where your models run: the LeanZero MLX engine on your Macs, and the cloud providers you've signed in to. Turn them into nodes under Nodes.",
  },
  sections: { id: 'providers.sections', defaultMessage: 'Providers sections' },
  tabMlx: { id: 'providers.tabMlx', defaultMessage: 'LeanZero MLX' },
  tabCloud: { id: 'leanzeroSwarm.tabCloud', defaultMessage: 'Cloud Providers' },
});

/**
 * How many nodes the swarm pool holds — the rows the Nodes page lists from config. No `swarm` key
 * is an empty pool (0); a read that failed is not a count, so it stays null and nothing is claimed.
 */
function useSwarmNodeCount(): number | null {
  const { read } = useConfig();
  const [count, setCount] = useState<number | null>(null);
  useEffect(() => {
    let alive = true;
    read('swarm', false)
      .then((raw) => {
        const devices = (raw as SwarmConfig | null)?.devices;
        if (alive) setCount(Array.isArray(devices) ? devices.length : 0);
      })
      .catch(() => alive && setCount(null));
    return () => {
      alive = false;
    };
  }, [read]);
  return count;
}

/**
 * Providers — where models come from (DESIGN-NODES-AND-STRATEGIES.md §5.4):
 *
 *   LeanZero MLX     — your Macs and the engine: Engine (with Run it) · My Macs · Models · Sampling,
 *                      with the setup strip (Macs → models → Run it → nodes) above them.
 *   Cloud Providers  — provider credentials and endpoints.
 *
 * Swarm Settings left for the Nodes page in the left nav, and My Macs moved inside LeanZero MLX
 * (Q-193, Q-194); their old `?tab=swarm` / `?tab=link` links redirect (providerRoutes.tsx).
 *
 * Both tab levels live in the URL (`?tab=mlx&mlx=models`), written with `replace` on every click, so
 * a deep link opens a tab and Back returns to the tab the person left (Q-203).
 */
const LeanZeroSwarmView: React.FC = () => {
  const intl = useIntl();
  const navigate = useNavigate();
  const nodeCount = useSwarmNodeCount();
  const [searchParams, setSearchParams] = useSearchParams();
  const tab = providersTabOf(searchParams.get('tab'));
  const mlxTab = mlxTabOf(searchParams.get('mlx'));

  const selectTab = useCallback(
    (next: ProvidersTab) =>
      setSearchParams(next === 'mlx' ? { tab: next, mlx: mlxTab } : { tab: next }, {
        replace: true,
      }),
    [mlxTab, setSearchParams]
  );
  const selectMlxTab = useCallback(
    (next: MlxTab) => setSearchParams({ tab: 'mlx', mlx: next }, { replace: true }),
    [setSearchParams]
  );

  const tabs: SegmentedOption<ProvidersTab>[] = [
    { value: 'mlx', label: intl.formatMessage(i18n.tabMlx) },
    { value: 'cloud', label: intl.formatMessage(i18n.tabCloud) },
  ];

  return (
    <MainPanelLayout>
      <MacsProvider>
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
              {tab === 'mlx' && (
                <MlxEngineView
                  tab={mlxTab}
                  onTabChange={selectMlxTab}
                  nodeCount={nodeCount}
                  onOpenNodes={() => navigate(NODES_ROUTE)}
                />
              )}
              {tab === 'cloud' && <CloudProvidersSection />}
            </ScrollArea>
          </div>
        </div>
      </MacsProvider>
    </MainPanelLayout>
  );
};

export default LeanZeroSwarmView;
