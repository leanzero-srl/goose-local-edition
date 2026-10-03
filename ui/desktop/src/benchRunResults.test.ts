import { describe, expect, it } from 'vitest';
import {
  benchRunKey,
  FORGE_NO_SURFACE_DETAIL,
  clipAbsence,
  parsePublishedIndex,
  resultDescribesRun,
  runResultFileName,
  treeVerdictDescribesRun,
} from './benchRunResults';

describe('every finished run keeps its own result', () => {
  it('keys a run by its engine id, or its start stamp before the id exists, as the view does', () => {
    expect(benchRunKey({ runId: 'cloud-07caff2d', startedAt: 'x' })).toBe('cloud-07caff2d');
    expect(benchRunKey({ runId: null, startedAt: '2026-10-03T08:00:00.000Z' })).toBe(
      'start-2026-10-03T08:00:00.000Z'
    );
    expect(runResultFileName('start-2026-10-03T08:00:00.000Z')).toBe(
      'start-2026-10-03T08_00_00.000Z.json'
    );
    expect(runResultFileName('../../etc/passwd')).toBe('.._.._etc_passwd.json');
  });

  it('attributes a stored row only to the run it names — never to a neighbour with another score', () => {
    const row = { runId: 'cloud-07caff2d', startedAt: '2026-10-03T08:00:00Z', score: 0.699 };
    expect(resultDescribesRun({ runId: 'cloud-07caff2d', score: 0.699 }, row)).toBe(true);
    expect(
      resultDescribesRun({ runMeta: { startedAt: '2026-10-03T08:00:00Z' }, score: 0.699 }, row)
    ).toBe(true);
    // The latest result.json belongs to the NEXT run: it must not lend itself.
    expect(resultDescribesRun({ runId: 'cloud-25eb9626', score: 0.0083 }, row)).toBe(false);
    expect(resultDescribesRun({ runId: 'cloud-07caff2d', score: 0.5 }, row)).toBe(false);
    expect(resultDescribesRun(null, row)).toBe(false);
    // A verdict in the run's own tree describes it when the stamped score agrees.
    expect(treeVerdictDescribesRun({ score: 0.699 }, row)).toBe(true);
    expect(treeVerdictDescribesRun({ score: 0.42 }, row)).toBe(false);
    expect(treeVerdictDescribesRun({}, row)).toBe(false);
  });

  it('reads the published index strictly — a malformed record is no record', () => {
    expect(
      parsePublishedIndex({
        a: { url: '/runs/1', title: 'T', score: 0.7, publishedAt: '2026-10-03T09:00:00Z' },
        b: { url: null, title: 'U', score: 0.1, publishedAt: '2026-10-03T09:00:00Z' },
        c: { title: 'no score' },
      })
    ).toEqual({
      a: { url: '/runs/1', title: 'T', score: 0.7, publishedAt: '2026-10-03T09:00:00Z' },
      b: { url: null, title: 'U', score: 0.1, publishedAt: '2026-10-03T09:00:00Z' },
    });
    expect(parsePublishedIndex(null)).toEqual({});
    expect(parsePublishedIndex([1])).toEqual({});
  });
});

describe('a result publishes without its clip ONLY on the site rule (website 9c371d7)', () => {
  // The REAL upstage/solar-mini4 rows (sb-7.2, 0.0083, tree openrouter-cloud-25eb9626-…-r0): ledgerd
  // never bound its port and the probe recorded nothing.
  const solar = [
    { check: 'server_runs', tier: 'A', score: 0.15, detail: 'process survives 5s without binding' },
    { check: 'serves_page', tier: 'A', score: 0.0, detail: 'GET / -> None' },
  ];
  it('Gauntlet: server_runs never bound AND serves_page 0 on "GET / -> None", nothing browser-graded above 0', () => {
    expect(clipAbsence('sb-7.2', solar)).toBe(
      'the app never served a page (server_runs: process survives 5s without binding; serves_page: GET / -> None)'
    );
    expect(
      clipAbsence('sb-7.1', [
        { check: 'server_runs', tier: 'A', score: 0, detail: 'crash at boot: ImportError' },
        { check: 'serves_page', tier: 'A', score: 0, detail: 'GET / -> None' },
      ])
    ).toMatch(/never served a page/);
  });
  it('Gauntlet: every other shape requires the clip', () => {
    const variants: unknown[][] = [
      [solar[0]], // serves_page missing
      [solar[1]], // server_runs missing
      [solar[0], { ...solar[1], detail: 'GET / -> 500' }],
      [solar[0], { ...solar[1], score: 0.1 }],
      [{ ...solar[0], score: 0.2 }, solar[1]],
      [{ ...solar[0], detail: 'process exited 1' }, solar[1]],
      [...solar, solar[1]], // a check name twice
      [...solar, { check: 'v_styling', tier: 'V', score: 0.2, detail: 'stylesheet=False' }],
    ];
    for (const rows of variants) expect(clipAbsence('sb-7.2', rows)).toBeNull();
    expect(clipAbsence('sb-7.0-rc', solar)).toBeNull();
    expect(clipAbsence('forge-1.0-rc', solar)).toBeNull();
    expect(clipAbsence('sb-7.2', undefined)).toBeNull();
  });
  it('the REAL solar-mini4 verdict fails the site rule: its J/V rows earned credit with no page', () => {
    // Measured 2026-10-03 on openrouter-cloud-25eb9626-…-r0/verdict.json — reported to the coordinator.
    const real = [
      ...solar,
      { check: 'j_console_clean', tier: 'J', score: 1, detail: '0 console error(s) across load+sync' },
      { check: 'j_notifications_feed', tier: 'J', score: 0.25, detail: 'partition state=None' },
      { check: 'v_styling', tier: 'V', score: 0.2, detail: "stylesheet=False, None backgrounds, font ''" },
    ];
    expect(clipAbsence('sb-7.2', real)).toBeNull();
  });
  it('Forge: the four visual rows all the scorer’s own "no surface rendered app content" zeros', () => {
    const rows = ['v_theme_tokens', 'v_dark_mode', 'v_csp_clean', 'v_console_clean'].map(
      (check) => ({
        check,
        tier: 'V',
        score: 0,
        detail: FORGE_NO_SURFACE_DETAIL,
      })
    );
    expect(FORGE_NO_SURFACE_DETAIL).toBe(
      'vacuous — precondition unmet: a surface rendered app content'
    );
    expect(clipAbsence('forge-1.0', rows)).toBe(
      'no surface rendered app content, so there was nothing to record'
    );
    expect(clipAbsence('forge-1.0', rows.slice(1))).toBeNull();
    expect(clipAbsence('forge-1.0', [{ ...rows[0], score: 0.5 }, ...rows.slice(1)])).toBeNull();
    expect(
      clipAbsence('forge-1.0', [{ ...rows[0], detail: 'other' }, ...rows.slice(1)])
    ).toBeNull();
  });
});
