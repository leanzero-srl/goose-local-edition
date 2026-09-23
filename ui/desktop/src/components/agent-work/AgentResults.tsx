import { AgentText } from './AgentText';
import { Bot, ExternalLink } from 'lucide-react';
import MarkdownContent from '../MarkdownContent';
import { ActivityDisclosure } from '../activity/ActivityDisclosure';
import { Chip, EmptyState, TNUM, TYPE, WEIGHT, cx } from '../lz';
import { isProtocolSafe } from '../../utils/urlSecurity';
import { fmtDuration, tickReport, type DeskModel } from './agentWorkModel';

/**
 * The viewed tick's result, read first: what the agent found (the worker's finding when one lane's
 * report was delivered as written, else the synthesis handoff), the next step it proposes, the
 * pages it read as links, and the evidence behind disclosures. One tick at a time — the tick the
 * URL opened, else the latest; earlier results are one click away in the tick navigator.
 */
export function AgentResults({ model }: { model: DeskModel }) {
  const t = model.viewTick;
  const tick = model.viewRecord;
  const isCurrent = t === model.tick;
  const attempted = model.tick > 0 || model.lanes.length > 0;
  const active = model.liveness !== 'stopped';
  const report = tick ? tickReport(tick) : null;
  const heading = (
    <h2 className={TYPE.h2} id="agent-result-heading">
      Result
    </h2>
  );

  if (!tick || (report?.mode === 'none' && !tick.outcome)) {
    return (
      <section aria-labelledby="agent-result-heading" data-testid="agent-result">
        {heading}
        {!attempted ? (
          <EmptyState
            icon={<Bot />}
            title="Ready for the first assignment"
            body="Run once to try the agent. Its findings and handoff appear here; live tool activity appears below."
          />
        ) : isCurrent ? (
          <EmptyState
            icon={<Bot />}
            title={active ? 'Working on the assignment' : 'Stopped without a result'}
            body={
              active
                ? 'The final handoff will appear here. Follow the lanes below while the agent works.'
                : 'This attempt did not record a final handoff. Its last activity is available below; run again to retry.'
            }
          />
        ) : (
          <EmptyState
            icon={<Bot />}
            title={`Tick ${t} is not in view`}
            body="The desk view reads the newest twelve tick records. Older ticks stay in the ledger files on disk."
          />
        )}
      </section>
    );
  }

  const r = report!;
  const lane = r.lane;
  const who = [
    typeof lane?.surgeon === 'string' ? `${lane.surgeon} lane` : '',
    typeof lane?.secs === 'number' ? fmtDuration(lane.secs * 1000) : '',
    typeof lane?.model === 'string' ? lane.model : '',
  ].filter(Boolean);

  return (
    <section
      aria-labelledby="agent-result-heading"
      data-testid="agent-result"
      className="flex min-w-0 flex-col gap-4"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        {heading}
        <span className={cx(TYPE.meta, TNUM)}>
          {r.mode === 'lane_report'
            ? `Worker report, delivered as written${who.length ? ` · ${who.join(' · ')}` : ''}`
            : r.mode === 'handoff'
              ? 'Synthesis handoff'
              : 'Tick summary'}
        </span>
        {r.confidence != null && (
          <Chip tone={r.confidence >= 2 ? 'ok' : 'warn'}>confidence {r.confidence}</Chip>
        )}
      </div>
      {tick.notes?.map((note, index) => (
        <p key={index} className={cx(TYPE.body, 'rounded-lz-control bg-lz-surface-2 px-3 py-2')}>
          {note}
        </p>
      ))}
      <div className="max-w-[72ch] text-lz-ink" data-testid="agent-result-lead">
        <MarkdownContent content={r.lead || 'No summary was recorded for this run.'} />
      </div>
      {r.nextStep && (
        <div className="max-w-[72ch]">
          <h3 className={cx(TYPE.meta, 'mb-1')}>Next step</h3>
          <MarkdownContent content={r.nextStep} />
        </div>
      )}
      {r.sources.length > 0 && (
        <div>
          <h3 className={cx(TYPE.meta, 'mb-1.5')}>Sources</h3>
          <ul className="flex flex-wrap gap-2" data-testid="agent-result-sources">
            {r.sources.map((url) => (
              <li key={url} className="min-w-0 max-w-full">
                <SourceLink url={url} />
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="flex max-w-[72ch] flex-col rounded-lz-card border border-lz-border">
        {(r.homework || r.evidence.length > 0) && (
          <ActivityDisclosure label="How it was checked">
            <div className="flex flex-col gap-3 px-4 pb-3">
              {r.homework && <AgentText text={r.homework} />}
              {r.evidence.length > 0 && (
                <ul className="flex list-disc flex-col gap-1.5 pl-5 text-sm">
                  {r.evidence.map((e, i) => (
                    <li key={i} className="min-w-0 [overflow-wrap:anywhere]">
                      {e}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </ActivityDisclosure>
        )}
        {r.mode === 'lane_report' && tick.synthesis?.handoff && (
          <ActivityDisclosure label="Full report">
            <div className="px-4 pb-3">
              <AgentText text={lane ? JSON.stringify(lane) : tick.synthesis.handoff} />
              <ActivityDisclosure label="Original handoff">
                <MarkdownContent content={tick.synthesis.handoff} />
              </ActivityDisclosure>
            </div>
          </ActivityDisclosure>
        )}
        {r.mode !== 'lane_report' && !!tick.synthesis?.facts.length && (
          <ActivityDisclosure label={`${tick.synthesis.facts.length} recorded findings`}>
            <ul className="space-y-2 px-4 pb-3 text-sm">
              {tick.synthesis.facts.map((fact, index) => (
                <li key={index}>
                  <AgentText text={fact} />
                </li>
              ))}
            </ul>
          </ActivityDisclosure>
        )}
        {!!tick.synthesis?.pending.length && (
          <ActivityDisclosure label="Pending for later ticks">
            <ul className="space-y-2 px-4 pb-3 text-sm">
              {tick.synthesis.pending.map((item, index) => (
                <li key={index}>
                  <AgentText text={item} />
                </li>
              ))}
            </ul>
          </ActivityDisclosure>
        )}
      </div>
    </section>
  );
}

function SourceLink({ url }: { url: string }) {
  let host = url;
  let rest = '';
  try {
    const u = new URL(url);
    host = u.host;
    rest = `${u.pathname === '/' ? '' : u.pathname}${u.search}`;
  } catch {
    // Not parseable as a URL: shown whole, still copyable.
  }
  return (
    <a
      href={url}
      title={url}
      target="_blank"
      rel="noopener noreferrer"
      onClick={(e) => {
        e.preventDefault();
        if (isProtocolSafe(url)) void window.electron.openExternal(url);
      }}
      className={cx(
        'inline-flex max-w-full items-center gap-1.5 rounded-lz-control border border-lz-border-strong px-2.5 py-1 text-lz-body hover:bg-lz-surface-2 [&_svg]:size-3.5 [&_svg]:shrink-0'
      )}
    >
      <span className={cx('shrink-0 text-lz-accent', WEIGHT.semibold)}>{host}</span>
      {rest && <span className="min-w-0 truncate text-lz-ink-2">{rest}</span>}
      <ExternalLink aria-hidden className="text-lz-ink-3" />
    </a>
  );
}
