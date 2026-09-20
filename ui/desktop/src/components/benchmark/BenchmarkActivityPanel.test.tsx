import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it } from 'vitest';
import { appendBenchmarkActivity, emptyBenchmarkActivity } from '../../benchActivity';
import { BenchmarkActivityPanel } from './BenchmarkActivityPanel';
afterEach(cleanup);
it('shows recorded actions and explicit phase while containing path and hiding raw code by default', () => {
  let activity = emptyBenchmarkActivity();
  for (const line of ['  ▸ edit', '    path web/viz.js', '}', 'BENCH_PHASE {"phase":"score"}'])
    activity = appendBenchmarkActivity(activity, line, 'stdout', 1000);
  const workdir = '/very/long/private/location/' + 'segment/'.repeat(30) + 'google-cloud-run-r0';
  render(
    <BenchmarkActivityPanel
      phase="score"
      activity={activity}
      now={3000}
      startedAt={1}
      workdir={workdir}
    />
  );
  expect(screen.getByText('Scorer running')).toBeVisible();
  expect(screen.getByText('Edit file')).toBeVisible();
  expect(screen.getByText('web/viz.js')).toBeVisible();
  expect(screen.getByText('Scoring started')).toBeVisible();
  expect(screen.queryByText(workdir)).toBeNull();
  expect(screen.queryByText('BENCH_PHASE', { exact: false })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Run location' }));
  expect(screen.getByText(workdir)).toHaveClass('break-all');
  fireEvent.click(screen.getByRole('button', { name: 'Console details' }));
  expect(screen.getByText(/BENCH_PHASE/)).toHaveClass('whitespace-pre-wrap', 'break-all');
});
it('restores snapshot observations and does not manufacture actions from empty output', () => {
  const { rerender } = render(
    <BenchmarkActivityPanel
      phase="build"
      activity={emptyBenchmarkActivity()}
      now={0}
      startedAt={null}
      workdir={null}
    />
  );
  expect(screen.getByText('No console output received yet.')).toBeVisible();
  expect(screen.queryByRole('list')).toBeNull();
  const saved = appendBenchmarkActivity(emptyBenchmarkActivity(), '  ▸ read_image', 'stdout', 1000);
  rerender(
    <BenchmarkActivityPanel
      phase="build"
      activity={saved}
      now={2000}
      startedAt={1}
      workdir="/run"
    />
  );
  expect(screen.getByText('Inspect image')).toBeVisible();
  expect(screen.getByText(/Latest action excerpts · 1/)).toBeVisible();
});

it('expands multiline command context instead of showing a shell prefix alone', () => {
  let activity = emptyBenchmarkActivity();
  for (const line of [
    '  ▸ shell',
    '    command: python3 -c "',
    "with open('SB7-CONTRACT.md') as f:",
    '    for line in f:',
    "        if line.startswith('#'):",
    '            print(line.rstrip())',
    '"',
    '',
  ])
    activity = appendBenchmarkActivity(activity, line, 'stdout', 1000);
  render(
    <BenchmarkActivityPanel
      phase="build"
      activity={activity}
      now={2000}
      startedAt={1}
      workdir="/run"
    />
  );
  expect(screen.getByText(/SB7-CONTRACT/)).toBeVisible();
  expect(screen.queryByText(/print\(line/)).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Expand action' }));
  expect(screen.getByText(/print\(line/)).toBeVisible();
});
