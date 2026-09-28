import { useState } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import type { MlxMountFitDto } from '@aaif/goose-sdk';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import { defineMessages, useIntl } from '../../i18n';
import { Button } from '../lz';
import { ToneBanner } from './studio';
import { RestoreActions, restoreRefusedByGate, useRestoreLine } from './MlxRestoreLine';
import { gbOf } from './mlxRestore';

/**
 * THE memory refusal on the Engine tab, once, in plain words (Q-277). On 3.0.66 one refusal was
 * stacked three times — Restore, Mount blocked, Mount failed — each carrying the gate's ~400-char
 * arithmetic, while the tile beside them said "Fits, 0.2 GB spare" off a later read: the banners
 * quoted the verdict of the mount that was refused, the tile the verdict judged now.
 *
 * Now the refusal is the status's last gate (`gateFit`, whichever path mounted: Run it, Restore,
 * Try again), and its figures are read from the SAME verdict the tile draws (`mountFit`) whenever
 * the tile shows that model — so the two cannot disagree. When memory has moved and that verdict
 * fits, the banner says so instead of a shortfall that is no longer true. The refusal's own words
 * (the rule's arithmetic, what Make room did) stay one click away under Details, and a restore
 * refused the same way lends this banner its Try again and Dismiss.
 */

const i18n = defineMessages({
  label: { id: 'mlxMountRefusal.label', defaultMessage: 'Did not start' },
  short: {
    id: 'mlxMountRefusal.short',
    defaultMessage:
      '{model} needs {need} GB on this Mac and {free} GB is free for it now — {short} GB short.',
  },
  fitsNow: {
    id: 'mlxMountRefusal.fitsNow',
    defaultMessage:
      '{model} did not fit on this Mac when it was asked to start. Memory has changed and it fits now — start it again.',
  },
  details: { id: 'mlxMountRefusal.details', defaultMessage: 'Details' },
  hideDetails: { id: 'mlxMountRefusal.hideDetails', defaultMessage: 'Hide details' },
});

/** The last gate when it refused: the refusal the Engine tab states. */
export function gateRefusalOf(
  status: Partial<Pick<MlxEngineStatus, 'gateFit'>> | null
): MlxMountFitDto | null {
  const gate = status?.gateFit;
  return gate != null && gate.verdict === 'block' ? gate : null;
}

/**
 * The verdict the refusal's figures come from: the tile's own (`mountFit`, judged now) when the
 * tile shows the refused model, else the refusal's.
 */
export function refusalVerdict(
  refused: MlxMountFitDto,
  live: MlxMountFitDto | null
): MlxMountFitDto {
  return live != null && live.modelId === refused.modelId ? live : refused;
}

export function MlxMountRefusalBanner({ status }: { status: MlxEngineStatus }) {
  const intl = useIntl();
  const line = useRestoreLine();
  const [open, setOpen] = useState(false);
  const refused = gateRefusalOf(status);
  if (refused == null) return null;
  const verdict = refusalVerdict(refused, status.mountFit ?? null);
  const model = refused.modelId.split('/').pop() || refused.modelId;
  const fitsNow = verdict.verdict !== 'block';
  const text = fitsNow
    ? intl.formatMessage(i18n.fitsNow, { model })
    : intl.formatMessage(i18n.short, {
        model,
        need: gbOf(verdict.needBytes),
        free: gbOf(verdict.budgetBytes),
        short: gbOf(verdict.shortBytes ?? verdict.needBytes - verdict.budgetBytes),
      });
  return (
    <div className="flex flex-col gap-2">
      <ToneBanner
        tone={fitsNow ? 'warn' : 'err'}
        label={intl.formatMessage(i18n.label)}
        text={text}
        testId="mlx-mount-refusal"
        action={
          <span className="flex shrink-0 items-center gap-2">
            <Button
              size="sm"
              variant="secondary"
              icon={open ? <ChevronUp /> : <ChevronDown />}
              onClick={() => setOpen((o) => !o)}
              aria-expanded={open}
              data-testid="mlx-mount-refusal-details"
            >
              {intl.formatMessage(open ? i18n.hideDetails : i18n.details)}
            </Button>
            {restoreRefusedByGate(line, refused.modelId) && <RestoreActions />}
          </span>
        }
      />
      {open && (
        <p className="break-all font-mono text-lz-mono" data-testid="mlx-mount-refusal-detail">
          {refused.message}
        </p>
      )}
    </div>
  );
}
