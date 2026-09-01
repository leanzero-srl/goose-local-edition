import React, { useState } from 'react';
import { Cpu } from 'lucide-react';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { ScrollArea } from '../ui/scroll-area';
import MlxEngineView from './MlxEngineView';
import CloudProvidersSection from './CloudProvidersSection';
import SwarmNodesSection from './SwarmNodesSection';
import LeanZeroLinkSection from './LeanZeroLinkSection';
import { AZURE } from './primitives';
import { defineMessages, useIntl } from '../../i18n';
import { useFeatures } from '../../contexts/FeaturesContext';

const i18n = defineMessages({
  subtitle: {
    id: 'leanzeroSwarm.subtitle',
    defaultMessage:
      'One place for the whole swarm: the in-house MLX engine, cloud provider credentials, and the node pool the swarm builds with.',
  },
  tabCloud: { id: 'leanzeroSwarm.tabCloud', defaultMessage: 'Cloud Providers' },
  tabSwarm: { id: 'leanzeroSwarm.tabSwarm', defaultMessage: 'Swarm Settings' },
  tabLink: { id: 'leanzeroSwarm.tabLink', defaultMessage: 'LeanZero Link' },
});

type SwarmTab = 'mlx' | 'cloud' | 'swarm' | 'link';

// Solid active fill with a fallback — this window also runs in builds without `.local-edition`,
// where a bare var() resolves to NOTHING and the active label silently vanishes (caught live).
const TOP_TAB_ACTIVE = AZURE;

/**
 * The "LeanZero Swarm" primary view — the single swarm/engine management surface:
 *
 *   LeanZero MLX     — the engine window (Engine / Models / Sampling sub-tabs), unchanged.
 *   Cloud Providers  — provider credentials, relocated from Settings, cloud-only.
 *   Swarm Settings   — NODES ONLY (owner amendment): add node, per-node provider, per-node
 *                      weight, remove. The full lever panel stays in Settings until the
 *                      golden-formula strip retires it.
 *
 * Benchmark register throughout: full borders, solid saturated fills, custom controls.
 */
const LeanZeroSwarmView: React.FC = () => {
  const intl = useIntl();
  const { leanzeroLink } = useFeatures();
  const [tab, setTab] = useState<SwarmTab>('mlx');

  const tabBtn = (t: SwarmTab, label: React.ReactNode) => {
    const active = tab === t;
    return (
      <button
        type="button"
        onClick={() => setTab(t)}
        className={`flex items-center gap-2 px-4 py-2 text-sm font-bold transition-colors ${
          active ? 'text-white' : 'bg-background-secondary text-text-secondary hover:text-text-primary'
        }`}
        style={active ? { backgroundColor: TOP_TAB_ACTIVE } : undefined}
        aria-pressed={active}
        data-testid={`leanzero-swarm-tab-${t}`}
      >
        {label}
      </button>
    );
  };

  return (
    <MainPanelLayout>
      <div className="flex-1 flex flex-col min-h-0">
        <div className="bg-background-primary px-8 pb-5 pt-16">
          <header className="flex flex-col page-transition border-b border-border-primary pb-5">
            <div className="flex items-center gap-3">
              <span
                className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded"
                style={{ backgroundColor: AZURE }}
              >
                <Cpu className="w-5 h-5 text-white" />
              </span>
              <h1 className="text-2xl font-bold text-text-primary">LeanZero Swarm</h1>
            </div>
            <p className="mt-1 max-w-[70ch] text-sm text-text-secondary">
              {intl.formatMessage(i18n.subtitle)}
            </p>
            <div className="mt-3 flex self-start overflow-hidden rounded border border-border-primary">
              {tabBtn('mlx', 'LeanZero MLX')}
              {tabBtn('cloud', intl.formatMessage(i18n.tabCloud))}
              {tabBtn('swarm', intl.formatMessage(i18n.tabSwarm))}
              {leanzeroLink && tabBtn('link', intl.formatMessage(i18n.tabLink))}
            </div>
          </header>
        </div>

        <div className="flex-1 min-h-0 relative px-8 pt-4">
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
