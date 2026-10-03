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

import solarVerdict from './components/benchmark/sb72-solar-noserve.fixture.json';

describe('a result publishes without its clip ONLY on the site rule (website 2900690)', () => {
  // The REAL upstage/solar-mini4 rows: sb-7.2, 0.0083 — ledgerd never bound its port, the probe recorded
  // nothing. 99 rows verbatim, including j_console_clean 1.0, j_notifications_feed 0.25, v_styling 0.2.
  const real = solarVerdict.checks;
  const solarLike = (overrides: Record<string, Record<string, unknown>> = {}) =>
    real.map((c) => ({ ...c, ...(overrides[c.check] ?? {}) }));

  it('the real solar verdict qualifies — the absence is the run’s result', () => {
    expect(solarVerdict.scorerVersion).toBe('sb-7.2');
    expect(solarVerdict.media).toBeNull();
    expect(clipAbsence('sb-7.2', real)).toBe(
      'the app never served a page (server_runs: process survives 5s without binding; serves_page: GET / -> None)'
    );
  });

  it('every clause is required: one changed row makes the clip mandatory again', () => {
    const changes: Record<string, Record<string, unknown>>[] = [
      { server_runs: { score: 0.2 } },
      { server_runs: { detail: 'process exited 1' } },
      { serves_page: { score: 0.1 } },
      { serves_page: { detail: 'GET / -> 500' } },
      { j_loads_data: { score: 0.5 } },
    ];
    for (const changed of changes) expect(clipAbsence('sb-7.2', solarLike(changed))).toBeNull();
    // No "never bound" row at all.
    expect(
      clipAbsence(
        'sb-7.2',
        real.map((c) =>
          c.detail.startsWith('not exercised: ledgerd never bound') ? { ...c, detail: 'x' } : c
        )
      )
    ).toBeNull();
    // A check name twice; a missing j_loads_data.
    expect(clipAbsence('sb-7.2', [...real, real[0]])).toBeNull();
    expect(
      clipAbsence(
        'sb-7.2',
        real.filter((c) => c.check !== 'j_loads_data')
      )
    ).toBeNull();
    // A crash at boot qualifies the same way as never binding.
    expect(
      clipAbsence(
        'sb-7.2',
        solarLike({ server_runs: { score: 0, detail: 'crash at boot: ImportError' } })
      )
    ).toMatch(/never served a page/);
    // Only the eras the site names.
    expect(clipAbsence('sb-7.0-rc', real)).toBeNull();
    expect(clipAbsence('forge-1.0-rc', real)).toBeNull();
    expect(clipAbsence('sb-7.2', undefined)).toBeNull();
  });

  it('Forge: the four vacuous visual rows AND every U and V row at 0', () => {
    const rows = [
      ...['v_theme_tokens', 'v_dark_mode', 'v_csp_clean', 'v_console_clean'].map((check) => ({
        check,
        tier: 'V',
        score: 0,
        detail: FORGE_NO_SURFACE_DETAIL,
      })),
      { check: 'u_widget_loads', tier: 'U', score: 0, detail: 'no widget' },
      { check: 'l_deployable', tier: 'L', score: 1, detail: 'lint: 0 errors' },
    ];
    expect(FORGE_NO_SURFACE_DETAIL).toBe(
      'vacuous — precondition unmet: a surface rendered app content'
    );
    expect(clipAbsence('forge-1.0', rows)).toBe(
      'no surface rendered app content, so there was nothing to record'
    );
    expect(clipAbsence('forge-1.0', rows.slice(1))).toBeNull();
    expect(
      clipAbsence('forge-1.0', [{ ...rows[0], detail: 'other' }, ...rows.slice(1)])
    ).toBeNull();
    // A UI row that earned anything means a surface rendered: a clip is required (retry, not clip-less).
    expect(
      clipAbsence('forge-1.0', [
        ...rows.slice(0, 4),
        { check: 'u_widget_loads', tier: 'U', score: 0.5 },
      ])
    ).toBeNull();
  });
});
