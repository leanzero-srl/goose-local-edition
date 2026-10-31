import { describe, expect, it } from 'vitest';
import {
  defaultFloorGb,
  draftsFromMemory,
  inferenceBoxCapGb,
  memoryConsequences,
  parseMemoryDrafts,
  presetDrafts,
  suggestedWiredMb,
  wiredDisableCommand,
  wiredFallbackCommand,
  wiredKeepMb,
  wiredLimitCheck,
  wiredLimitCommand,
  wiredLimitValid,
  wiredMaxMb,
  type MemoryDrafts,
} from './memoryTunables';
import { sanitizeSettingsForWrite } from './MlxEngineView';

const GIB = 1024 ** 3;
const MAC_128 = 128 * GIB;
const drafts = (over: Partial<MemoryDrafts>): MemoryDrafts => ({
  ...draftsFromMemory({}),
  ...over,
});

describe('memory tunable drafts', () => {
  it('round-trips every set value and leaves blanks off the payload', () => {
    const memory = {
      metalLimitGb: 112,
      memoryFloorGb: 4,
      prefixCacheGb: 10,
      prefillStepSize: 512,
      maxContextTokens: 262144,
      guardMarginGb: 1.5,
      maxConcurrentRequests: 4,
      autoUtilizationFloor: 0.85,
      autoUtilizationCeiling: 0.95,
      autoHeadroomFraction: 0.1,
      systemReserveFraction: 0.05,
    };
    const parsed = parseMemoryDrafts(draftsFromMemory(memory), {
      physicalBytes: MAC_128,
      kvBytesPerToken: null,
    });
    expect(parsed.problems).toEqual([]);
    expect(parsed.memory).toEqual(memory);
    expect(
      parseMemoryDrafts(draftsFromMemory({}), { physicalBytes: MAC_128, kvBytesPerToken: null })
        .memory
    ).toEqual({});
  });

  it('refuses cap + floor over physical RAM', () => {
    const p = parseMemoryDrafts(drafts({ metalLimitGb: '130', memoryFloorGb: '11' }), {
      physicalBytes: MAC_128,
      kvBytesPerToken: null,
    });
    expect(p.problems).toContainEqual(
      expect.objectContaining({ kind: 'capPlusFloor', capGb: 130, floorGb: 11 })
    );
  });

  it('checks the cap against the default floor goose sends when the floor is blank', () => {
    const p = parseMemoryDrafts(drafts({ metalLimitGb: '133' }), {
      physicalBytes: MAC_128,
      kvBytesPerToken: null,
    });
    expect(p.problems).toContainEqual(
      expect.objectContaining({ kind: 'capPlusFloor', capGb: 133, floorGb: 7 })
    );
    const ok = parseMemoryDrafts(drafts({ metalLimitGb: '130' }), {
      physicalBytes: MAC_128,
      kvBytesPerToken: null,
    });
    expect(ok.problems.filter((x) => x.kind === 'capPlusFloor')).toEqual([]);
  });

  it('refuses an auto floor above the ceiling, against the engine default for the unset side', () => {
    const p = parseMemoryDrafts(drafts({ autoUtilizationFloor: '0.98' }), {
      physicalBytes: MAC_128,
      kvBytesPerToken: null,
    });
    expect(p.problems).toContainEqual({ kind: 'floorAboveCeiling', floor: 0.98, ceiling: 0.97 });
  });

  it('refuses a window whose KV alone exceeds the cap only once KV per token is known', () => {
    const d = drafts({ metalLimitGb: '40', maxContextTokens: '1000000' });
    expect(
      parseMemoryDrafts(d, { physicalBytes: MAC_128, kvBytesPerToken: null }).problems
    ).toEqual([]);
    const p = parseMemoryDrafts(d, { physicalBytes: MAC_128, kvBytesPerToken: 65536 });
    expect(p.problems[0]).toMatchObject({ kind: 'kvOverCap', tokens: 1000000 });
  });

  it('names out-of-range, non-integer and non-number fields', () => {
    const p = parseMemoryDrafts(
      drafts({
        metalLimitGb: '0',
        systemReserveFraction: '0.6',
        prefillStepSize: '1.5',
        guardMarginGb: 'x',
      }),
      { physicalBytes: MAC_128, kvBytesPerToken: null }
    );
    expect(p.problems).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ kind: 'range', key: 'metalLimitGb' }),
        expect.objectContaining({ kind: 'range', key: 'systemReserveFraction', max: 0.5 }),
        { kind: 'notInteger', key: 'prefillStepSize' },
        { kind: 'notNumber', key: 'guardMarginGb' },
      ])
    );
  });
});

describe('derived consequences', () => {
  it('estimates the context the cap holds from the live KV per token', () => {
    const c = memoryConsequences(
      { metalLimitGb: 112 },
      {
        physicalBytes: MAC_128,
        workingSetBytes: 103e9,
        budget: { kvBytesPerToken: 100_000, weightsBytes: 60e9, prefixBudgetBytes: 10e9 },
      }
    );
    // (112 - 60 - 10 - 2% of 112) GB / 100 kB
    expect(c.maxContextTokens).toBe(Math.floor((112e9 - 60e9 - 10e9 - 2.24e9) / 100_000));
    expect(c.freeForMacGb).toBeCloseTo(MAC_128 / 1e9 - 112, 5);
    expect(c.capAboveWorkingSet).toBe(true);
  });

  it('uses the engine resolved cap while the field is auto', () => {
    const c = memoryConsequences(
      {},
      { physicalBytes: MAC_128, workingSetBytes: null, budget: { limitBytes: 90e9 } }
    );
    expect(c.capGb).toBe(90);
    expect(c.capFromEngine).toBe(true);
    expect(c.maxContextTokens).toBeNull();
  });
});

describe('presets only fill fields', () => {
  it('derives the floor and the inference box cap per machine', () => {
    expect(defaultFloorGb(128 * GIB)).toBe(7);
    expect(defaultFloorGb(96 * GIB)).toBe(5);
    expect(defaultFloorGb(64 * GIB)).toBe(3);
    expect(inferenceBoxCapGb(128 * GIB)).toBe(112);
    expect(inferenceBoxCapGb(96 * GIB)).toBe(80);
    const base = drafts({ prefillStepSize: '512' });
    expect(presetDrafts('workstation', base, MAC_128)).toMatchObject({
      metalLimitGb: '',
      memoryFloorGb: '11',
      prefillStepSize: '512',
    });
    expect(presetDrafts('inferenceBox', base, MAC_128)).toMatchObject({
      metalLimitGb: '112',
      memoryFloorGb: '4',
    });
  });
});

describe('the wired limit', () => {
  const GIB_ = 1024 ** 3;
  const R = 16;
  // The same table as goose-sidecar wired_limit.rs the_reserve_math_on_64_96_and_128_gb_macs.
  it.each([
    [64, 8, 49_152, 41_000],
    [96, 10, 81_920, 72_000],
    [128, 10, 114_688, 105_000],
  ])('%i GiB with %i GB in use: max %i MB, suggestion %i MB', (gib, othersGb, max, suggestion) => {
    const physical = gib * GIB_;
    expect(wiredMaxMb(physical, R)).toBe(max);
    expect(suggestedWiredMb(physical, othersGb * 1e9, R)).toBe(suggestion);
    expect(wiredLimitCheck(suggestion, physical, R, othersGb * 1e9).kind).toBe('ok');
    expect(wiredLimitCheck(max, physical, R, othersGb * 1e9).kind).toBe('belowOthers');
    expect(wiredLimitCheck(max + 1, physical, R, null).kind).toBe('belowReserve');
    expect(wiredKeepMb(0, physical)).toBeNull();
  });

  it("refuses the owner's first number below the reserve, and validates 0 or 8192..physical", () => {
    expect(wiredKeepMb(122000, MAC_128)).toBe(9072);
    expect(wiredLimitCheck(122000, MAC_128, R, null)).toEqual({ kind: 'belowReserve', keepMb: 9072 });
    expect(wiredLimitValid(122000, MAC_128, R)).toBe(false);
    expect(wiredLimitValid(122000, MAC_128, 8)).toBe(true);
    expect(wiredLimitValid(0, MAC_128, R)).toBe(true);
    expect(wiredLimitValid(8192, MAC_128, R)).toBe(true);
    expect(wiredLimitValid(8191, MAC_128, R)).toBe(false);
    expect(wiredLimitValid(131073, MAC_128, 0)).toBe(false);
    expect(wiredLimitValid(9000.5, MAC_128, R)).toBe(false);
    expect(suggestedWiredMb(16 * GIB_, 4 * GIB_, R)).toBeNull();
  });

  it('builds the enable, disable and fallback commands the backend runs', () => {
    expect(wiredLimitCommand(110000)).toBe('/usr/sbin/sysctl iogpu.wired_limit_mb=110000');
    expect(wiredDisableCommand(false)).toBe('/usr/sbin/sysctl iogpu.wired_limit_mb=0');
    expect(wiredDisableCommand(true)).toBe(
      "/usr/sbin/sysctl iogpu.wired_limit_mb=0 && /bin/launchctl bootout system/net.leanzero.iogpu-wired-limit 2>/dev/null; /bin/rm -f '/Library/LaunchDaemons/net.leanzero.iogpu-wired-limit.plist'"
    );
    expect(wiredFallbackCommand(110000)).toBe('sudo /usr/sbin/sysctl iogpu.wired_limit_mb=110000');
  });
});

describe('settings writes keep the memory tunables', () => {
  it('a sampling or folder save never drops them', () => {
    const next = sanitizeSettingsForWrite({
      modelsDir: '~/.goose/models',
      port: 8090,
      spawnCommand: [],
      modelProfiles: {},
      memory: { metalLimitGb: 112 },
    });
    expect(next.memory).toEqual({ metalLimitGb: 112 });
  });
});
