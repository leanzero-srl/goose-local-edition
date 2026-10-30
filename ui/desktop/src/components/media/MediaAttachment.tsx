import { useRef, useState } from 'react';
import { cx } from '../lz';

/**
 * An attached audio or video file: a solid kind badge, the file name, and a play/pause control
 * the app draws itself (video shows its frame and plays in place). Used in the composer tray and in
 * the sent message.
 */
export function MediaAttachment({
  kind,
  src,
  name,
  mimeType,
  compact = false,
}: {
  kind: 'audio' | 'video';
  src?: string;
  name?: string | null;
  mimeType: string;
  compact?: boolean;
}) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const [playing, setPlaying] = useState(false);
  const label = name || (kind === 'audio' ? 'Audio clip' : 'Video clip');

  const toggle = () => {
    const el = kind === 'audio' ? audioRef.current : videoRef.current;
    if (!el) return;
    if (el.paused) {
      void el.play();
    } else {
      el.pause();
    }
  };

  const badge = (
    <span
      data-testid={`media-badge-${kind}`}
      className="inline-flex items-center justify-center rounded px-1.5 py-0.5 text-lz-meta font-bold uppercase text-white"
      style={{ background: kind === 'audio' ? 'var(--color-node-5)' : 'var(--color-node-2)' }}
    >
      {kind}
    </span>
  );

  return (
    <div
      data-testid="media-attachment"
      data-kind={kind}
      className={cx(
        'flex flex-col gap-1.5 rounded-lg border border-lz-border bg-lz-surface-2 p-2',
        compact ? 'w-[200px]' : 'w-[280px]'
      )}
    >
      {kind === 'video' && src && (
        <video
          ref={videoRef}
          src={src}
          className="w-full rounded bg-black"
          onPlay={() => setPlaying(true)}
          onPause={() => setPlaying(false)}
          onEnded={() => setPlaying(false)}
          onClick={toggle}
          preload="metadata"
          playsInline
        />
      )}
      {kind === 'audio' && src && (
        <audio
          ref={audioRef}
          src={src}
          onPlay={() => setPlaying(true)}
          onPause={() => setPlaying(false)}
          onEnded={() => setPlaying(false)}
          preload="metadata"
        />
      )}
      <div className="flex items-center gap-2 min-w-0">
        {badge}
        <span className="flex-1 truncate text-lz-body text-lz-ink" title={label}>
          {label}
        </span>
        {src && (
          <button
            type="button"
            onClick={toggle}
            data-testid="media-play"
            aria-label={playing ? 'Pause' : 'Play'}
            className="flex h-7 w-7 items-center justify-center rounded-full bg-lz-accent text-lz-accent-ink"
          >
            {playing ? (
              <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                <rect x="2" y="1" width="3" height="10" fill="currentColor" />
                <rect x="7" y="1" width="3" height="10" fill="currentColor" />
              </svg>
            ) : (
              <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                <path d="M3 1 L11 6 L3 11 Z" fill="currentColor" />
              </svg>
            )}
          </button>
        )}
      </div>
      <span className="text-lz-meta text-lz-ink-3 truncate">{mimeType}</span>
    </div>
  );
}
