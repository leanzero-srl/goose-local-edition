/* global HTMLVideoElement */
import { useRef, useState } from 'react';
import { Button, TYPE, cx } from '../lz';

export interface BenchmarkVideoEvidence {
  url: string;
  caption: string;
  sha256: string;
  bytes: number;
}

export function BenchmarkVideo({ video }: { video: BenchmarkVideoEvidence }) {
  const player = useRef<HTMLVideoElement>(null);
  const [playing, setPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const toggle = async () => {
    if (!player.current) return;
    if (playing) player.current.pause();
    else {
      try {
        await player.current.play();
      } catch {
        setError('This recording could not be played.');
      }
    }
  };
  return (
    <figure className="flex flex-col gap-3">
      <video
        ref={player}
        src={video.url}
        preload="metadata"
        muted
        playsInline
        aria-label={video.caption}
        className="w-full rounded-lz-card bg-black"
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => setPlaying(false)}
        onLoadedMetadata={() => setDuration(player.current?.duration ?? 0)}
        onTimeUpdate={() => setPosition(player.current?.currentTime ?? 0)}
        onError={() => setError('The recorded video is unavailable or cannot be decoded.')}
      />
      <figcaption className={TYPE.body}>{video.caption}</figcaption>
      <div className="flex items-center gap-2">
        <Button onClick={() => void toggle()}>
          {playing ? 'Pause recording' : 'Play recording'}
        </Button>
        <Button
          onClick={() => {
            if (player.current) player.current.currentTime = 0;
          }}
        >
          Replay from start
        </Button>
        <span className={TYPE.meta}>
          {Math.floor(position)} / {Number.isFinite(duration) ? Math.floor(duration) : '—'} seconds
        </span>
      </div>
      <p className={cx(TYPE.meta, 'break-all')}>
        Same graded browser session · SHA-256 {video.sha256}
      </p>
      {error && (
        <p role="status" className="text-lz-err">
          {error}
        </p>
      )}
    </figure>
  );
}
