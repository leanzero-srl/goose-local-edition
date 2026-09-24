import { describe, expect, it } from 'vitest';
import { createIntl } from 'react-intl';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { Mac } from './macs';
import { SELF_KEY } from './macs';
import {
  macLine,
  macStateWord,
  macTrayText,
  macsTrayOpenLabel,
  summarizeMac,
  type SummaryInput,
} from './macSummary';
import { FLASH_READY, HOSTING_RANK_1, STOPPED_WITH_CONFIG } from './mlxDistributed.fixtures';

const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });

const SELF: Mac = {
  key: SELF_KEY,
  isSelf: true,
  nodeId: 'self-node',
  name: 'Mihai’s MacBook',
  hostname: 'mihai-mbp',
  meshIp: null,
  online: true,
  sessionsActive: 0,
  allows: null,
  pollError: null,
};
const STUDIO: Mac = { ...SELF, key: 'studio-1', isSelf: false, name: 'Work’s Mac Studio' };

const status = (overrides: Partial<MlxEngineStatus>): MlxEngineStatus => ({
  state: 'running',
  modelId: 'Mihai-LeanZero/Qwen3.8-27B',
  restartRequired: false,
  availableMemoryGb: 60,
  totalMemoryGb: 128,
  ...overrides,
});

const input = (overrides: Partial<SummaryInput>): SummaryInput => ({
  status: null,
  statusError: null,
  activity: null,
  decodeTps: null,
  ...overrides,
});

describe('what a Mac is doing, in one vocabulary and the engine palette', () => {
  it('offline and switched-off Macs claim no palette colour', () => {
    const offline = summarizeMac({ ...STUDIO, online: false, pollError: 'timed out' }, input({}));
    expect(offline).toMatchObject({ phase: null, state: 'offline', detail: 'timed out' });
    expect(macLine(intl, offline)).toBe('Not reachable over LeanZero Link');

    const off = summarizeMac(
      { ...STUDIO, allows: { manage_models: false, answer_chat: true, run_split: true } },
      input({ status: status({}) })
    );
    expect(off).toMatchObject({ phase: null, state: 'off' });
    expect(macLine(intl, off)).toBeNull();
    expect(macStateWord(intl, off.state)).toBe('Off');
  });

  it('not read yet is Checking; a failed read is Can’t read in red with goose’s words', () => {
    expect(summarizeMac(STUDIO, input({}))).toMatchObject({ phase: null, state: 'checking' });
    const unreadable = summarizeMac(STUDIO, input({ statusError: 'connection refused' }));
    expect(unreadable).toMatchObject({
      phase: 'failed',
      state: 'unreadable',
      detail: 'connection refused',
    });
    expect(macStateWord(intl, unreadable.state)).toBe('Can’t read');
  });

  it('stopped is Not loaded (dark outline); mounting is Loading (amber); failed is red with the reason', () => {
    const stopped = summarizeMac(
      SELF,
      input({ status: status({ state: 'stopped', modelId: undefined }) })
    );
    expect(stopped).toMatchObject({ phase: 'unloaded', state: 'notLoaded' });
    expect(macLine(intl, stopped)).toBe('No model loaded');

    const mounting = summarizeMac(SELF, input({ status: status({ state: 'mounting' }) }));
    expect(mounting).toMatchObject({ phase: 'loading', state: 'loading' });
    expect(macLine(intl, mounting)).toBe('Loading Qwen3.8-27B');

    const failed = summarizeMac(
      SELF,
      input({ status: status({ state: 'failed', lastError: 'out of memory' }) })
    );
    expect(failed).toMatchObject({ phase: 'failed', state: 'failed' });
    expect(macLine(intl, failed)).toBe('Qwen3.8-27B failed: out of memory');
  });

  it('running: idle grey, reading blue, writing green with its live rate, held orange', () => {
    const running = status({});
    expect(summarizeMac(SELF, input({ status: running }))).toMatchObject({
      phase: 'idle',
      state: 'idle',
    });
    const reading = summarizeMac(SELF, input({ status: running, activity: 'prefill' }));
    expect(reading).toMatchObject({ phase: 'reading', state: 'reading' });
    expect(macLine(intl, reading)).toBe('Qwen3.8-27B');

    const writing = summarizeMac(
      SELF,
      input({ status: running, activity: 'generating', decodeTps: 21.94 })
    );
    expect(writing).toMatchObject({ phase: 'writing', state: 'writing', decodeTps: 21.94 });
    expect(macLine(intl, writing)).toBe('Qwen3.8-27B · 21.9 tok/s');

    const held = summarizeMac(SELF, input({ status: running, activity: 'queued' }));
    expect(held).toMatchObject({ phase: 'held', state: 'held' });
    expect(macStateWord(intl, held.state)).toBe('Held');
  });

  it('a peer’s activity is never borrowed from this Mac’s live read', () => {
    const peer = summarizeMac(
      STUDIO,
      input({ status: status({}), activity: 'generating', decodeTps: 30 })
    );
    expect(peer).toMatchObject({ phase: 'idle', state: 'idle', decodeTps: null });
  });

  it('this Mac running a split says so; this Mac serving a rank names whose split', () => {
    const split = summarizeMac(SELF, input({ status: status({}), distributed: FLASH_READY }));
    expect(split.state).toBe('split');
    expect(split.splitCount).toBe(FLASH_READY.nodes.length);
    expect(macLine(intl, split)).toMatch(/split across 2 Macs$/);

    const stoppedSplit = summarizeMac(
      SELF,
      input({ status: status({ state: 'stopped' }), distributed: STOPPED_WITH_CONFIG })
    );
    expect(stoppedSplit.state).toBe('notLoaded');

    const hosting = summarizeMac(SELF, input({ status: status({}), distributed: HOSTING_RANK_1 }));
    expect(hosting.state).toBe('hosting');
    expect(macLine(intl, hosting)).toContain(
      `part of ${HOSTING_RANK_1.hosting!.requesterName}’s split`
    );
  });
});

describe('the tray says what the card says', () => {
  it('name — the card’s line; a Mac with no line gets its state word', () => {
    const writing = summarizeMac(
      SELF,
      input({ status: status({}), activity: 'generating', decodeTps: 21.9 })
    );
    expect(macTrayText(intl, SELF, writing)).toBe('Mihai’s MacBook — Qwen3.8-27B · 21.9 tok/s');
    const off = summarizeMac(
      { ...STUDIO, allows: { manage_models: false, answer_chat: false, run_split: false } },
      input({})
    );
    expect(macTrayText(intl, STUDIO, off)).toBe('Work’s Mac Studio — Off');
    expect(macsTrayOpenLabel(intl)).toBe('Open My Macs');
  });
});
