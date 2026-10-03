import { useEffect, useState } from 'react';
import { TYPE } from '../lz';
import { BenchmarkVideo, type BenchmarkVideoEvidence } from './BenchmarkVideo';

/**
 * The run's graded browser clip. `absence` is the scorer's own reason no browser session could be
 * recorded (clipAbsence: the app never served a page / no surface rendered) — stated as the run's
 * result, never as a missing file.
 */
export function RunVideoEvidence({
  workdir,
  absence,
}: {
  workdir: string | undefined;
  absence?: string | null;
}) {
  const [result, setResult] = useState<{ videos: BenchmarkVideoEvidence[]; error?: string } | null>(
    null
  );
  useEffect(() => {
    let current = true;
    setResult(null);
    if (!workdir) {
      setResult({ videos: [], error: 'This result has no recorded run directory.' });
      return;
    }
    void window.electron
      .benchmarkMedia(workdir)
      .then((value) => {
        if (current) setResult(value);
      })
      .catch((error) => {
        if (current) setResult({ videos: [], error: String(error) });
      });
    return () => {
      current = false;
    };
  }, [workdir]);
  if (!result) return <p className={TYPE.bodyMuted}>Loading the graded browser recording…</p>;
  if (!result.videos.length && absence)
    return (
      <p role="status" data-testid="recording-absent" className={TYPE.body}>
        No recording: {absence}.
      </p>
    );
  if (result.error || !result.videos.length)
    return (
      <p role="status" className={TYPE.bodyMuted}>
        Graded browser video unavailable: {result.error ?? 'the run recorded no video.'}
      </p>
    );
  return (
    <div className="flex flex-col gap-6">
      {result.videos.map((video) => (
        <BenchmarkVideo key={video.sha256} video={video} />
      ))}
    </div>
  );
}
