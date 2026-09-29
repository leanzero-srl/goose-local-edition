import { useState } from 'react';
import { FileWarning, FolderOpen, Archive } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import type { UnreadableConfigFile } from '../../acp/config';
import { useConfig } from '../ConfigContext';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { Button, SPACE, SURFACE, TONE_FILL, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';

const i18n = defineMessages({
  headline: {
    id: 'unreadableConfigBanner.headline',
    defaultMessage:
      '{role, select, secrets {goose could not read the file that holds your keys} other {goose could not read your settings file}}',
  },
  consequence: {
    id: 'unreadableConfigBanner.consequence',
    defaultMessage:
      '{role, select, secrets {Until it is fixed, goose runs without the keys saved in it, so providers that need one look signed out.} other {Until it is fixed, goose runs on its defaults, so your providers, models and extensions can look gone.}} Nothing is saved over the file while it cannot be read.',
  },
  place: {
    id: 'unreadableConfigBanner.place',
    defaultMessage: 'Line {line}, column {column}',
  },
  showFile: { id: 'unreadableConfigBanner.showFile', defaultMessage: 'Reveal in Finder' },
  moveAside: {
    id: 'unreadableConfigBanner.moveAside',
    defaultMessage: 'Move it aside and start fresh',
  },
  confirmTitle: {
    id: 'unreadableConfigBanner.confirmTitle',
    defaultMessage: 'Move {fileName} aside?',
  },
  confirmMessage: {
    id: 'unreadableConfigBanner.confirmMessage',
    defaultMessage:
      'goose renames it to {fileName}.corrupt- followed by the date and time, in the same folder, so nothing in it is deleted, then starts {role, select, secrets {with no saved keys} other {a fresh settings file}}. You can copy what you need back from the renamed file.',
  },
  confirm: { id: 'unreadableConfigBanner.confirm', defaultMessage: 'Move it aside' },
  cancel: { id: 'unreadableConfigBanner.cancel', defaultMessage: 'Cancel' },
  moveFailed: {
    id: 'unreadableConfigBanner.moveFailed',
    defaultMessage: 'goose could not move it aside: {error}',
  },
});

function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

function UnreadableFileCard({ file }: { file: UnreadableConfigFile }) {
  const intl = useIntl();
  const { moveConfigAside } = useConfig();
  const [confirming, setConfirming] = useState(false);
  const [moving, setMoving] = useState(false);
  const [moveError, setMoveError] = useState<string | null>(null);
  const fileName = fileNameOf(file.path);

  const moveAside = async () => {
    setMoving(true);
    setMoveError(null);
    try {
      await moveConfigAside(file.path);
      setConfirming(false);
    } catch (error) {
      setMoveError(error instanceof Error ? error.message : String(error));
      setConfirming(false);
    } finally {
      setMoving(false);
    }
  };

  return (
    <div
      role="alert"
      data-testid="unreadable-config-banner"
      data-role={file.role}
      className={cx(SURFACE.overlay, 'flex flex-col overflow-hidden')}
    >
      <div className={cx(TONE_FILL.err, 'flex items-start gap-2.5 px-lz-card py-2.5')}>
        <FileWarning aria-hidden className="mt-0.5 size-5 shrink-0" />
        <p className={cx('min-w-0 flex-1 text-lz-body', WEIGHT.semibold)}>
          {intl.formatMessage(i18n.headline, { role: file.role })}
        </p>
      </div>
      <div className={cx(SPACE.card, 'flex flex-col gap-2.5')}>
        <p data-testid="unreadable-config-path" className={cx(TYPE.mono, 'break-all')}>
          {file.path}
        </p>
        <p data-testid="unreadable-config-reason" className={cx(TYPE.body, 'break-words')}>
          {file.line != null && file.column != null && (
            <span className={cx(TONE_TEXT.err, WEIGHT.semibold)}>
              {intl.formatMessage(i18n.place, { line: file.line, column: file.column })}
              {' · '}
            </span>
          )}
          {file.reason}
        </p>
        <p className={TYPE.bodyMuted}>
          {intl.formatMessage(i18n.consequence, { role: file.role })}
        </p>
        {moveError != null && (
          <p data-testid="unreadable-config-move-error" className={cx(TYPE.body, TONE_TEXT.err)}>
            {intl.formatMessage(i18n.moveFailed, { error: moveError })}
          </p>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="secondary"
            size="sm"
            icon={<FolderOpen />}
            data-testid="unreadable-config-show"
            onClick={() => void window.electron.revealInFinder(file.path)}
          >
            {intl.formatMessage(i18n.showFile)}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            icon={<Archive />}
            data-testid="unreadable-config-move-aside"
            disabled={moving}
            onClick={() => setConfirming(true)}
          >
            {intl.formatMessage(i18n.moveAside)}
          </Button>
        </div>
      </div>
      <ConfirmationModal
        isOpen={confirming}
        title={intl.formatMessage(i18n.confirmTitle, { fileName })}
        message={intl.formatMessage(i18n.confirmMessage, { role: file.role, fileName })}
        detail={file.path}
        confirmLabel={intl.formatMessage(i18n.confirm)}
        cancelLabel={intl.formatMessage(i18n.cancel)}
        confirmVariant="destructive"
        isSubmitting={moving}
        onConfirm={() => void moveAside()}
        onCancel={() => setConfirming(false)}
      />
    </div>
  );
}

/**
 * Q-468: goosed boots past a settings file it cannot parse and saves refuse to write over it
 * (Q-465). Without this the person saw an empty setup and no reason; now each unreadable file is
 * named, with the parser's own error and place, and the two ways out.
 */
export default function UnreadableConfigBanner() {
  const { unreadableFiles } = useConfig();
  if (unreadableFiles.length === 0) return null;
  return (
    // Q-471: a row of the app's column, never an overlay — it pushes the route down so nothing it
    // would cover (onboarding's cards, its own reason text) is hidden. pt-8 clears the 32px
    // titlebar drag strip; the height cap keeps the route below reachable when both files fail.
    <div
      data-testid="unreadable-config-banners"
      className="relative w-full shrink-0 overflow-y-auto px-4 pb-2 pt-8 max-h-[50%]"
    >
      <div className="mx-auto flex w-full max-w-[640px] flex-col gap-2">
        {unreadableFiles.map((file) => (
          <UnreadableFileCard key={file.path} file={file} />
        ))}
      </div>
    </div>
  );
}
