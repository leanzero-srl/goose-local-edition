import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Gauge } from 'lucide-react';
import { EngineGlanceCard, StageIcon, stageWord } from './EngineGlanceCard';
import { setGlancePrefs, useEngineGlance } from './glanceStore';
import { dockRestorable, dockShown } from '../../utils/engineGlanceRules';
import { nodeHref } from '../../utils/navigationUtils';
import { defineMessages, useIntl } from '../../i18n';
import {
  listedTitleOf,
  sessionHref,
  useListedNamesVersion,
} from '../sessionActivity/sessionActivityStore';
import { useFormingOf } from '../forming/formingStore';
import { FOCUS, MOTION, PHASE_FILL, RADIUS, ROW, SURFACE, WEIGHT, cx } from '../lz';

/**
 * The engine glance inside a goose window: the card at the FOOT of the sidebar, between the
 * sessions and the bottom block. It is its own item of the sidebar's column, so the sessions' scroll
 * area ends where it begins and nothing expanding above ever draws under it (Q-216). There is
 * no floating card over the content any more (Q-217); the desktop mini window is for goose in the
 * background. Hidden by the person, it leaves one row that brings it back (Q-218).
 */

const i18n = defineMessages({
  restore: { id: 'engineGlance.restore', defaultMessage: 'Show the engine card' },
  restoreTitle: {
    id: 'engineGlance.restoreTitle',
    defaultMessage: 'The engine card is hidden. Show it again at the foot of the sidebar.',
  },
});

/**
 * The slot's own item in the column, under a hairline: the sessions above scroll and are cut at a
 * visible edge, instead of reading as if they ran on under the card (Q-216). In a short window the
 * card gives way — the sessions keep their fifth of the column (NavigationPanel) and Settings stays
 * on screen, and the card scrolls inside its slot (measured in Chromium: at 420 px the old
 * full-height card pushed Settings out of the frame and left the sessions 0 px).
 */
const DOCK_SLOT = cx('min-h-0 shrink overflow-y-auto border-t px-2 py-2', SURFACE.hairline);

function useOpeners() {
  const navigate = useNavigate();
  return {
    openEngine: () => navigate('/leanzero-swarm?tab=mlx'),
    openSession: (sessionId: string) => navigate(sessionHref(sessionId)),
    openNode: (nodeId: string) => navigate(nodeHref(nodeId)),
  };
}

/** The docked card, or — hidden by the person — the one row that brings it back. */
export function EngineGlanceDockSlot() {
  const push = useEngineGlance();
  useListedNamesVersion();
  const [expanded, setExpanded] = useState(false);
  const { openEngine, openSession, openNode } = useOpeners();
  const forming = useFormingOf(push?.engine.chat?.sessionId ?? null);

  if (!push) return null;
  if (dockRestorable(push)) return <EngineGlanceRestore />;
  if (!dockShown(push)) return null;
  return (
    <div data-testid="engine-glance-dock" className={DOCK_SLOT}>
      <EngineGlanceCard
        push={push}
        variant="dock"
        collapsed={false}
        expanded={expanded}
        onOpenEngine={openEngine}
        onOpenSession={openSession}
        onOpenNode={openNode}
        onToggleExpanded={() => setExpanded((v) => !v)}
        onCollapsedChange={() => undefined}
        onHide={() => void setGlancePrefs({ ...push.prefs, inApp: false })}
        forming={forming}
        chatName={listedTitleOf}
      />
    </div>
  );
}

/**
 * Where the card was: one sidebar row that says what the engine is doing in its phase colour and
 * brings the card back. It shows only while the card would have something to show — a row that
 * restores an empty card would read as broken — and Settings › App carries the same switch.
 */
function EngineGlanceRestore() {
  const intl = useIntl();
  const push = useEngineGlance();
  if (!push) return null;
  const engine = push.engine;
  return (
    <div data-testid="engine-glance-restore-slot" className={DOCK_SLOT}>
      <button
        type="button"
        data-testid="engine-glance-restore"
        title={intl.formatMessage(i18n.restoreTitle)}
        onClick={() => void setGlancePrefs({ ...push.prefs, inApp: true })}
        className={cx(
          'flex w-full min-w-0 items-center gap-2 px-2 text-left text-lz-body text-lz-ink [&_svg]:size-4 [&_svg]:shrink-0',
          ROW.dense,
          RADIUS.control,
          SURFACE.hover,
          FOCUS,
          MOTION
        )}
      >
        <Gauge aria-hidden />
        <span className={cx('min-w-0 flex-1 truncate', WEIGHT.semibold)}>
          {intl.formatMessage(i18n.restore)}
        </span>
        {engine.present && (
          // The stage as a solid badge in the engine's phase colour; its word is read out and shown
          // on hover — beside the label it would truncate one or the other in a 240 px sidebar.
          <span
            data-testid="engine-glance-restore-stage"
            data-phase={engine.phase}
            title={stageWord(intl, engine.stage)}
            className={cx(
              'inline-flex size-6 shrink-0 items-center justify-center [&_svg]:size-3.5',
              RADIUS.pill,
              PHASE_FILL[engine.phase]
            )}
          >
            <StageIcon stage={engine.stage} />
            <span className="sr-only">{stageWord(intl, engine.stage)}</span>
          </span>
        )}
      </button>
    </div>
  );
}
