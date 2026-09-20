import { render, screen, fireEvent } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { BenchmarkVideo } from './BenchmarkVideo';
it('plays the verified recording with custom controls and exposes decoding failures', async () => {
  const video = {
    url: 'http://127.0.0.1:1234/token',
    caption: 'Same graded payment scene',
    sha256: 'abc123',
    bytes: 100,
  };
  render(<BenchmarkVideo video={video} />);
  const element = screen.getByLabelText(video.caption);
  const play = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(element, 'play', { value: play });
  expect(element).not.toHaveAttribute('controls');
  fireEvent.click(screen.getByRole('button', { name: 'Play recording' }));
  expect(play).toHaveBeenCalledOnce();
  fireEvent.play(element);
  expect(screen.getByRole('button', { name: 'Pause recording' })).toBeInTheDocument();
  fireEvent.error(element);
  expect(screen.getByRole('status')).toHaveTextContent('cannot be decoded');
  expect(screen.getByText(/SHA-256 abc123/)).toBeInTheDocument();
});
