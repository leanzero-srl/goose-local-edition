import MarkdownContent from '../MarkdownContent';
import { cutStopFromRecord, type SplitRecord } from '../chatServedBy/splitRecord';
import { SplitStoppedNotice } from './NoNodeNotice';
import { useLatestMlxDistributedStatus } from './mlxMount';
import type { NetworkCut } from './parseNetworkCut';

/**
 * A stream cut while the split served (Q-122): when the split's record — saved with the message,
 * or the live events after the turn began — says the split stopped under it, the message's error
 * IS the split-stop notice and "Network error: Stream decode error …" goes behind Details. When no
 * stop is known the error is shown as goose wrote it: the cause is not invented.
 */
export default function SplitCutNotice({
  cut,
  record,
  hasAnswer,
  live,
  retryText,
  onRetry,
}: {
  cut: NetworkCut;
  record: SplitRecord;
  hasAnswer: boolean;
  live: boolean;
  retryText: string | null;
  onRetry: (text: string) => void;
}) {
  const distributed = useLatestMlxDistributedStatus();
  const stop = cutStopFromRecord(record, distributed);
  if (!stop) {
    return (
      <div data-testid="network-cut-raw" className={hasAnswer ? 'mt-2' : undefined}>
        <MarkdownContent content={cut.raw} />
      </div>
    );
  }
  return (
    <SplitStoppedNotice
      stop={stop}
      rows={[]}
      hasAnswer={hasAnswer}
      cut
      errorText={cut.raw}
      live={live}
      back={false}
      retryText={retryText}
      onRetry={onRetry}
    />
  );
}
