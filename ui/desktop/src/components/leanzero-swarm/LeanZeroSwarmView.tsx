import React, { useState } from 'react';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { ScrollArea } from '../ui/scroll-area';
import MlxEngineView from './MlxEngineView';
import CloudProvidersSection from './CloudProvidersSection';
import SwarmNodesSection from './SwarmNodesSection';
import LeanZeroLinkSection from './LeanZeroLinkSection';
import { PageHeader, SURFACE, Segmented, cx, type SegmentedOption } from '../lz';
import { defineMessages, useIntl } from '../../i18n';
import { useFeatures } from '../../contexts/FeaturesContext';

const i18n = defineMessages({
  subtitle: {
    id: 'leanzeroSwarm.subtitle',
    defaultMessage:
      'One place for the whole flock: the in-house MLX engine, cloud provider credentials, and the node pool the flock builds with.',
  },
  tabCloud: { id: 'leanzeroSwarm.tabCloud', defaultMessage: 'Cloud Providers' },
  tabSwarm: { id: 'leanzeroSwarm.tabSwarm', defaultMessage: 'Flock Settings' },
  tabLink: { id: 'leanzeroSwarm.tabLink', defaultMessage: 'LeanZero Link' },
});

type SwarmTab = 'mlx' | 'cloud' | 'swarm' | 'link';

/**
 * The "LeanZero Flock" primary view — the single swarm/engine management surface:
 *
 *   LeanZero MLX     — the engine window (Engine / Models / Sampling sub-tabs), unchanged.
 *   Cloud Providers  — provider credentials, relocated from Settings, cloud-only.
 *   Swarm Settings   — NODES ONLY (owner amendment): add node, per-node provider, per-node
 *                      weight, remove. The full lever panel stays in Settings until the
 *                      golden-formula strip retires it.
 *
 * LeanZero Studio register: a PageHeader with the section Segmented in its actions slot, the
 * page surface underneath. Solid colour with meaning, one accent, custom controls only.
 */
const LeanZeroSwarmView: React.FC = () => {
  const intl = useIntl();
  const { leanzeroLink } = useFeatures();
  const [tab, setTab] = useState<SwarmTab>('mlx');

  const tabs: SegmentedOption<SwarmTab>[] = [
    { value: 'mlx', label: 'LeanZero MLX' },
    { value: 'cloud', label: intl.formatMessage(i18n.tabCloud) },
    { value: 'swarm', label: intl.formatMessage(i18n.tabSwarm) },
    ...(leanzeroLink
      ? [{ value: 'link' as const, label: intl.formatMessage(i18n.tabLink) }]
      : []),
  ];

  return (
    <MainPanelLayout>
      <div className={cx('flex min-h-0 flex-1 flex-col', SURFACE.page)}>
        <div className={cx('border-b px-lz-page pb-6 pt-16', SURFACE.hairline)}>
          <PageHeader
            className="page-transition"
            title="LeanZero Flock"
            subtitle={<span className="block max-w-[70ch]">{intl.formatMessage(i18n.subtitle)}</span>}
            actions={
              <Segmented
                aria-label="LeanZero Flock sections"
                options={tabs}
                value={tab}
                onChange={setTab}
              />
            }
          />
        </div>

        <div className="relative min-h-0 flex-1 px-lz-page pt-6">
          <ScrollArea className="h-full">
            {tab === 'mlx' && <MlxEngineView />}
            {tab === 'cloud' && <CloudProvidersSection />}
            {tab === 'swarm' && <SwarmNodesSection onOpenCloudProviders={() => setTab('cloud')} />}
            {tab === 'link' && leanzeroLink && <LeanZeroLinkSection />}
          </ScrollArea>
        </div>
      </div>
    </MainPanelLayout>
  );
};

export default LeanZeroSwarmView;
