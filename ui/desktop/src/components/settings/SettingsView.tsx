import * as TabsPrimitive from '@radix-ui/react-tabs';
import { ScrollArea } from '../ui/scroll-area';
import { View, ViewOptions } from '../../utils/navigationUtils';
import ExternalBackendSection from './app/ExternalBackendSection';
import AppSettingsSection from './app/AppSettingsSection';
import ConfigSettings from './config/ConfigSettings';
import PromptsSettingsSection from './PromptsSettingsSection';
import type { ExtensionConfig } from '../../types/extensions';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import {
  Share2,
  Monitor,
  MessageSquare,
  FileText,
  Keyboard,
  KeyRound,
  DownloadCloud,
} from 'lucide-react';
import ImportView from './import/ImportView';
import { useState, useEffect, useRef } from 'react';
import ChatSettingsSection from './chat/ChatSettingsSection';
import KeyboardShortcutsSection from './keyboard/KeyboardShortcutsSection';
import AuthSettingsSection from './auth/AuthSettingsSection';
import { CONFIGURATION_ENABLED } from '../../updates';
import { trackSettingsTabViewed } from '../../utils/analytics';
import { useEdition } from '../../contexts/EditionContext';
import { defineMessages, useIntl } from '../../i18n';
import { FOCUS, MOTION, PageHeader, RADIUS, SectionHeader, SPACE, SURFACE, cx } from '../lz';

const i18n = defineMessages({
  title: {
    id: 'settingsView.title',
    defaultMessage: 'Settings',
  },
  tabChat: {
    id: 'settingsView.tabChat',
    defaultMessage: 'Chat',
  },
  tabSession: {
    id: 'settingsView.tabSession',
    defaultMessage: 'Session',
  },
  tabPrompts: {
    id: 'settingsView.tabPrompts',
    defaultMessage: 'Prompts',
  },
  tabKeyboard: {
    id: 'settingsView.tabKeyboard',
    defaultMessage: 'Keyboard',
  },
  tabAuth: {
    id: 'settingsView.tabAuth',
    defaultMessage: 'Auth',
  },
  tabApp: {
    id: 'settingsView.tabApp',
    defaultMessage: 'App',
  },
});

export type SettingsViewOptions = {
  deepLinkConfig?: ExtensionConfig;
  showEnvVars?: boolean;
  section?: string;
};

/**
 * The tab strip is the lz Segmented's class recipe on Radix tab triggers: the triggers keep the
 * tablist/tab/tabpanel semantics, the `settings-*-tab` test ids the e2e journey selects, and the
 * `data-state` the tab test pins — the primitive's radio options carry none of those, so the
 * strip is composed here from the same tokens.
 */
const STRIP = cx(
  'inline-flex max-w-full items-center gap-0.5 self-start overflow-x-auto bg-lz-surface p-0.5',
  SURFACE.outline,
  RADIUS.control
);
const SEGMENT =
  'inline-flex h-7 items-center gap-1.5 whitespace-nowrap rounded-[4px] px-2.5 text-[12px] font-lz-medium [&_svg]:size-3.5 [&_svg]:shrink-0';
function segmentClass(active: boolean): string {
  return cx(
    SEGMENT,
    active ? SURFACE.selected : cx('text-lz-ink-2 hover:text-lz-ink', SURFACE.hover),
    FOCUS,
    MOTION
  );
}

const TAB_PANEL = 'outline-none pb-lz-page';

export default function SettingsView({
  onClose,
  viewOptions,
}: {
  onClose: () => void;
  // Kept in the props contract for callers; unused since the Models tab left the settings nav.
  setView?: (view: View, viewOptions?: ViewOptions) => void;
  viewOptions: SettingsViewOptions;
}) {
  const hasTrackedInitialTab = useRef(false);
  const { edition } = useEdition();
  const isLocalEdition = edition === 'local';
  // The Models tab left the settings nav in pass A (provider management consolidates into the
  // LeanZero Swarm view), and the owner removed the Swarm LeanZero tab too — the LeanZero Swarm
  // view's nodes tab is the only swarm surface now. SwarmSettingsSection stays in code, unrouted.
  const [activeTab, setActiveTab] = useState('chat');
  const intl = useIntl();

  const handleTabChange = (tab: string) => {
    setActiveTab(tab);
    trackSettingsTabViewed(tab);
  };

  // Determine initial tab based on section prop
  useEffect(() => {
    if (viewOptions.section) {
      // Map section names to tab values. 'models' deep links land on the nearest surviving surface
      // now that the Models and Swarm tabs are gone.
      const sectionToTab: Record<string, string> = {
        update: 'app',
        models: 'chat',
        modes: 'chat',
        sharing: 'sharing',
        styles: 'chat',
        tools: 'chat',
        app: 'app',
        chat: 'chat',
        prompts: 'prompts',
        keyboard: 'keyboard',
        auth: 'auth',
      };

      const targetTab = sectionToTab[viewOptions.section];
      if (targetTab) {
        setActiveTab(targetTab);
      }
    }
  }, [viewOptions.section]);

  // Reset active tab if the Import tab (local edition only) becomes unavailable
  useEffect(() => {
    if (!isLocalEdition && activeTab === 'import') {
      setActiveTab('chat');
    }
  }, [isLocalEdition, activeTab]);

  useEffect(() => {
    if (!hasTrackedInitialTab.current) {
      trackSettingsTabViewed(activeTab);
      hasTrackedInitialTab.current = true;
    }
  }, [activeTab]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !event.defaultPrevented) {
        onClose();
      }
    };

    document.addEventListener('keydown', handleKeyDown);

    return () => {
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [onClose]);

  const tabChat = intl.formatMessage(i18n.tabChat);
  const tabSession = intl.formatMessage(i18n.tabSession);
  const tabPrompts = intl.formatMessage(i18n.tabPrompts);
  const tabKeyboard = intl.formatMessage(i18n.tabKeyboard);
  const tabAuth = intl.formatMessage(i18n.tabAuth);
  const tabApp = intl.formatMessage(i18n.tabApp);

  return (
    <>
      <MainPanelLayout>
        <div className={cx('flex min-h-0 flex-1 flex-col', SURFACE.page)}>
          <div className={cx('pb-6 pt-lz-page', SPACE.pageX)}>
            <PageHeader title={intl.formatMessage(i18n.title)} />
          </div>

          <div className={cx('relative min-h-0 flex-1', SPACE.pageX)}>
            <TabsPrimitive.Root
              value={activeTab}
              onValueChange={handleTabChange}
              className="flex h-full flex-col"
            >
              <TabsPrimitive.List aria-label={intl.formatMessage(i18n.title)} className={STRIP}>
                {isLocalEdition && (
                  <TabsPrimitive.Trigger
                    value="import"
                    className={segmentClass(activeTab === 'import')}
                    data-testid="settings-import-tab"
                  >
                    <DownloadCloud aria-hidden />
                    Import
                  </TabsPrimitive.Trigger>
                )}
                <TabsPrimitive.Trigger
                  value="chat"
                  className={segmentClass(activeTab === 'chat')}
                  data-testid="settings-chat-tab"
                >
                  <MessageSquare aria-hidden />
                  {tabChat}
                </TabsPrimitive.Trigger>
                <TabsPrimitive.Trigger
                  value="sharing"
                  className={segmentClass(activeTab === 'sharing')}
                  data-testid="settings-sharing-tab"
                >
                  <Share2 aria-hidden />
                  {tabSession}
                </TabsPrimitive.Trigger>
                <TabsPrimitive.Trigger
                  value="prompts"
                  className={segmentClass(activeTab === 'prompts')}
                  data-testid="settings-prompts-tab"
                >
                  <FileText aria-hidden />
                  {tabPrompts}
                </TabsPrimitive.Trigger>
                <TabsPrimitive.Trigger
                  value="keyboard"
                  className={segmentClass(activeTab === 'keyboard')}
                  data-testid="settings-keyboard-tab"
                >
                  <Keyboard aria-hidden />
                  {tabKeyboard}
                </TabsPrimitive.Trigger>
                <TabsPrimitive.Trigger
                  value="auth"
                  className={segmentClass(activeTab === 'auth')}
                  data-testid="settings-auth-tab"
                >
                  <KeyRound aria-hidden />
                  {tabAuth}
                </TabsPrimitive.Trigger>
                <TabsPrimitive.Trigger
                  value="app"
                  className={segmentClass(activeTab === 'app')}
                  data-testid="settings-app-tab"
                >
                  <Monitor aria-hidden />
                  {tabApp}
                </TabsPrimitive.Trigger>
              </TabsPrimitive.List>

              <ScrollArea className="mt-4 flex-1">
                {isLocalEdition && (
                  <TabsPrimitive.Content value="import" className={TAB_PANEL}>
                    <SectionHeader title="Import" className="mb-2" />
                    <ImportView />
                  </TabsPrimitive.Content>
                )}

                <TabsPrimitive.Content value="chat" className={TAB_PANEL}>
                  <SectionHeader title={tabChat} className="mb-2" />
                  <ChatSettingsSection />
                </TabsPrimitive.Content>

                <TabsPrimitive.Content value="sharing" className={TAB_PANEL}>
                  <SectionHeader title={tabSession} className="mb-2" />
                  <div className="space-y-8 pb-8">
                    <ExternalBackendSection />
                  </div>
                </TabsPrimitive.Content>

                <TabsPrimitive.Content value="prompts" className={TAB_PANEL}>
                  <SectionHeader title={tabPrompts} className="mb-2" />
                  <PromptsSettingsSection />
                </TabsPrimitive.Content>

                <TabsPrimitive.Content value="keyboard" className={TAB_PANEL}>
                  <SectionHeader title={tabKeyboard} className="mb-2" />
                  <KeyboardShortcutsSection />
                </TabsPrimitive.Content>

                <TabsPrimitive.Content value="auth" className={TAB_PANEL}>
                  <SectionHeader title={tabAuth} className="mb-2" />
                  <AuthSettingsSection />
                </TabsPrimitive.Content>

                <TabsPrimitive.Content value="app" className={TAB_PANEL}>
                  <SectionHeader title={tabApp} className="mb-2" />
                  <div className="space-y-8">
                    {CONFIGURATION_ENABLED && <ConfigSettings />}
                    <AppSettingsSection scrollToSection={viewOptions.section} />
                  </div>
                </TabsPrimitive.Content>
              </ScrollArea>
            </TabsPrimitive.Root>
          </div>
        </div>
      </MainPanelLayout>
    </>
  );
}
