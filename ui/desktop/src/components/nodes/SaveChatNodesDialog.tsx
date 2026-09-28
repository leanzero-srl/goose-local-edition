import { useState } from 'react';
import { Loader2, X } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { INPUT } from '../leanzero-swarm/studio';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { refreshGlanceNodes } from '../engineGlance/glanceStore';
import { nodesWrite } from '../../acp/nodes';
import type { NodesConfig } from './model';

/**
 * "Save as a strategy…" (Q-359): the chat's own node set gets the name the person types and stops
 * being the chat's alone (`chat` cleared). It is the same strategy, so the chat keeps running on it;
 * from now on it is listed under Strategies, where new chats and swarm builds can use it. Stored
 * through the one write door; its refusals are shown in goosed's words.
 */

const i18n = defineMessages({
  title: { id: 'chatNodes.saveTitle', defaultMessage: 'Save this chat’s nodes as a strategy' },
  body: {
    id: 'chatNodes.saveBody',
    defaultMessage:
      'This chat keeps running on it. It is listed under Strategies, where new chats and swarm builds can use it too.',
  },
  name: { id: 'chatNodes.saveName', defaultMessage: 'Name' },
  save: { id: 'chatNodes.save', defaultMessage: 'Save as a strategy' },
  cancel: { id: 'chatNodes.cancel', defaultMessage: 'Cancel' },
  close: { id: 'chatNodes.close', defaultMessage: 'Close' },
  refused: { id: 'chatNodes.notSaved', defaultMessage: 'Not saved' },
  failed: { id: 'chatNodes.saveFailed', defaultMessage: 'The strategy could not be saved' },
  emptyName: { id: 'chatNodes.saveEmptyName', defaultMessage: 'Give it a name to save it' },
});

export interface SaveChatNodesDialogProps {
  config: NodesConfig;
  /** The chat's set, by its strategy id. */
  strategyId: string;
  onClose: () => void;
}

export function SaveChatNodesDialog({ config, strategyId, onClose }: SaveChatNodesDialogProps) {
  const intl = useIntl();
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [refusals, setRefusals] = useState<string[]>([]);
  const trimmed = name.trim();

  const save = async () => {
    setBusy(true);
    setRefusals([]);
    try {
      const strategies = (config.strategies ?? []).map((s) =>
        s.id === strategyId ? { ...s, name: trimmed, chat: undefined } : s
      );
      const response = await nodesWrite({ ...config, strategies });
      if (response.written) {
        onClose();
        return;
      }
      setRefusals((response.refusals ?? []).map((r) => r.message));
    } catch (e) {
      setRefusals([mlxErrorMessage(e, intl.formatMessage(i18n.failed))]);
    } finally {
      setBusy(false);
      refreshGlanceNodes();
    }
  };

  return (
    <OverlayDialog
      open
      onClose={onClose}
      panelClassName={cx('flex w-[30rem] flex-col gap-4 p-5', SURFACE.overlay)}
    >
      <div className="flex items-start justify-between gap-3" data-testid="save-chat-nodes-dialog">
        <div className="flex min-w-0 flex-col gap-1">
          <OverlayDialogTitle asChild>
            <h2 className={cx('break-words', TYPE.h2)}>{intl.formatMessage(i18n.title)}</h2>
          </OverlayDialogTitle>
          <p className={cx('break-words', TYPE.bodyMuted)}>{intl.formatMessage(i18n.body)}</p>
        </div>
        <Button
          variant="ghost"
          size="sm"
          iconOnly
          icon={<X />}
          aria-label={intl.formatMessage(i18n.close)}
          onClick={onClose}
        />
      </div>
      <label className="flex flex-col gap-1">
        <span className={TYPE.meta}>{intl.formatMessage(i18n.name)}</span>
        <input
          className={cx(INPUT, 'w-full')}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && trimmed && !busy) void save();
          }}
          data-testid="save-chat-nodes-name"
        />
      </label>
      {refusals.length > 0 && (
        <div role="alert" className="flex flex-col gap-1" data-testid="save-chat-nodes-refusals">
          <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
            {intl.formatMessage(i18n.refused)}
          </span>
          {refusals.map((r) => (
            <p key={r} className={cx('break-words', TYPE.body)}>
              {r}
            </p>
          ))}
        </div>
      )}
      <div className="flex flex-wrap items-center justify-end gap-2">
        {!trimmed && (
          <span className={cx('mr-auto', TYPE.meta, WEIGHT.semibold)}>
            {intl.formatMessage(i18n.emptyName)}
          </span>
        )}
        <Button variant="ghost" onClick={onClose}>
          {intl.formatMessage(i18n.cancel)}
        </Button>
        <Button
          variant="primary"
          disabled={!trimmed || busy}
          icon={busy ? <Loader2 className="animate-spin" /> : undefined}
          onClick={() => void save()}
          data-testid="save-chat-nodes-save"
        >
          {intl.formatMessage(i18n.save)}
        </Button>
      </div>
    </OverlayDialog>
  );
}
