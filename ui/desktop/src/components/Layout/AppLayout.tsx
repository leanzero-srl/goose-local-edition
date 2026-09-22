import { RouteFlight } from './RouteFlight';
import React, { useEffect, useState } from 'react';
import { IpcRendererEvent } from 'electron';
import { Outlet, useLocation } from 'react-router-dom';
import { motion } from 'framer-motion';
import { Menu, PanelLeft } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button } from '../ui/button';
import ChatSessionsContainer from '../ChatSessionsContainer';
import { useChatContext } from '../../contexts/ChatContext';
import { NavigationProvider, useNavigationContext } from './NavigationContext';
import { Navigation } from './NavigationPanel';
import { NAV_DIMENSIONS, Z_INDEX } from './constants';
import { cn } from '../../utils';
import { RADIUS, SURFACE, cx } from '../lz/tokens';
import { UserInput } from '../../types/message';

const i18n = defineMessages({
  openNavigation: {
    id: 'appLayout.openNavigation',
    defaultMessage: 'Open navigation',
  },
  collapseNavigation: {
    id: 'appLayout.collapseNavigation',
    defaultMessage: 'Collapse navigation',
  },
});

interface AppLayoutContentProps {
  activeSessions: Array<{
    sessionId: string;
    initialMessage?: UserInput;
    noAutoSubmit?: boolean;
  }>;
}

const AppLayoutContent: React.FC<AppLayoutContentProps> = ({ activeSessions }) => {
  const intl = useIntl();
  const location = useLocation();
  const safeIsMacOS = (window?.electron?.platform || 'darwin') === 'darwin';
  const chatContext = useChatContext();
  const isOnPairRoute = location.pathname === '/pair';

  const [isFullScreen, setIsFullScreen] = useState(false);

  useEffect(() => {
    if (!safeIsMacOS) return;
    window.electron
      .getIsFullScreen()
      .then(setIsFullScreen)
      .catch(() => {});
    const handler = (_event: IpcRendererEvent, ...args: unknown[]) => {
      setIsFullScreen(Boolean(args[0]));
    };
    window.electron.on('fullscreen-change', handler);
    return () => window.electron.off('fullscreen-change', handler);
  }, [safeIsMacOS]);

  const { isNavExpanded, setIsNavExpanded, navWidth, setNavWidth } = useNavigationContext();
  const [dragging, setDragging] = useState(false);

  // The sidebar's right edge is a drag handle (ChatGPT's sidebar resizes the same way): pointer
  // capture keeps the drag alive off the handle; the width is clamped and remembered by the context.
  const startResize = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = navWidth;
    const handle = e.currentTarget;
    handle.setPointerCapture(e.pointerId);
    setDragging(true);
    const onMove = (ev: PointerEvent) => setNavWidth(startWidth + (ev.clientX - startX));
    const onUp = () => {
      setDragging(false);
      handle.removeEventListener('pointermove', onMove);
      handle.removeEventListener('pointerup', onUp);
      handle.removeEventListener('pointercancel', onUp);
    };
    handle.addEventListener('pointermove', onMove);
    handle.addEventListener('pointerup', onUp);
    handle.addEventListener('pointercancel', onUp);
  };

  if (!chatContext) {
    throw new Error('AppLayoutContent must be used within ChatProvider');
  }

  const { setChat } = chatContext;

  const needsTrafficLightInset = safeIsMacOS && !isFullScreen;
  const headerPadding = needsTrafficLightInset ? 'pl-[96px]' : 'pl-4';
  const headerTop = needsTrafficLightInset ? 'top-[14px]' : 'top-[11px]';
  const navToggleTitle = intl.formatMessage(
    isNavExpanded ? i18n.collapseNavigation : i18n.openNavigation
  );

  return (
    <div className="flex flex-1 w-full h-full relative animate-fade-in bg-background-primary flex-row">
      <div
        style={{ zIndex: Z_INDEX.HEADER }}
        className={cn('absolute flex items-center gap-1', headerPadding, headerTop, 'ml-1.5')}
      >
        <Button
          onClick={() => setIsNavExpanded(!isNavExpanded)}
          className="no-drag hover:!bg-background-tertiary"
          variant="ghost"
          size="xs"
          title={navToggleTitle}
          aria-label={navToggleTitle}
        >
          {isNavExpanded ? <PanelLeft className="w-5 h-5" /> : <Menu className="w-5 h-5" />}
        </Button>
      </div>

      {/* Main content with navigation. Shared canvas; the sidebar is the Studio card frame —
          the lz hairline at the card radius — floating on it with breathing room. */}
      <div className="flex flex-1 w-full h-full min-h-0 flex-row">
        <motion.div
          key="nav"
          initial={false}
          animate={{ width: isNavExpanded ? navWidth : 0 }}
          transition={dragging ? { duration: 0 } : { type: 'spring', stiffness: 400, damping: 40 }}
          style={{ height: '100%' }}
          className="relative flex-shrink-0 overflow-hidden h-full p-2"
        >
          <div
            data-testid="nav-frame"
            className={cx('h-full w-full overflow-hidden border', SURFACE.hairline, RADIUS.card)}
          >
            <Navigation />
          </div>
          {isNavExpanded && (
            <div
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize navigation"
              aria-valuemin={NAV_DIMENSIONS.NAV_MIN_WIDTH}
              aria-valuemax={NAV_DIMENSIONS.NAV_MAX_WIDTH}
              aria-valuenow={navWidth}
              data-testid="nav-resize-handle"
              onPointerDown={startResize}
              onDoubleClick={() => setNavWidth(NAV_DIMENSIONS.NAV_WIDTH)}
              className={cx(
                'absolute inset-y-2 right-0 w-2 cursor-col-resize select-none',
                dragging ? 'bg-lz-accent' : 'hover:bg-lz-accent'
              )}
            />
          )}
        </motion.div>

        {/* Main content — no border / no card; just flows on the canvas. */}
        <div className="relative flex-1 overflow-hidden min-h-0">
          <RouteFlight />
          <Outlet />
          {/* Always render ChatSessionsContainer to keep SSE connections alive.
              When navigating away from /pair, hide it with CSS */}
          <div className={isOnPairRoute ? 'contents' : 'hidden'}>
            <ChatSessionsContainer setChat={setChat} activeSessions={activeSessions} />
          </div>
        </div>
      </div>
    </div>
  );
};

interface AppLayoutProps {
  activeSessions: Array<{
    sessionId: string;
    initialMessage?: UserInput;
    noAutoSubmit?: boolean;
  }>;
}

export const AppLayout: React.FC<AppLayoutProps> = ({ activeSessions }) => {
  return (
    <NavigationProvider>
      <AppLayoutContent activeSessions={activeSessions} />
    </NavigationProvider>
  );
};
