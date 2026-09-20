import { Bot } from 'lucide-react';
import MarkdownContent from '../MarkdownContent';
import { ActivityDisclosure } from '../activity/ActivityDisclosure';
import { EmptyState, Panel } from '../lz';
import type { DeskModel } from './agentWorkModel';

export function AgentResults({ model }: { model: DeskModel }) {
  const ticks = [...model.ticks].sort((a, b) => b.tick - a.tick);
  const attempted = model.tick > 0 || model.lanes.length > 0;
  const active = model.liveness !== 'stopped';
  const missingCurrentResult = attempted && !ticks.some((tick) => tick.tick === model.tick);
  return (
    <Panel title="Conversation & results">
      {missingCurrentResult && (
        <EmptyState
          icon={<Bot />}
          title={active ? 'Working on the assignment' : 'Stopped without a result'}
          body={
            active
              ? 'The final handoff will appear here. Follow the activity below while the agent works.'
              : 'This attempt did not record a final handoff. Its last activity is available below; run again to retry.'
          }
        />
      )}
      {ticks.length === 0 && !attempted ? (
        <EmptyState
          icon={<Bot />}
          title="Ready for the first assignment"
          body="Run once to try the agent. Its findings and handoff appear here; live tool activity appears below."
        />
      ) : ticks.length > 0 ? (
        <div className="space-y-6">
          {ticks.map((tick) => {
            const source = tick.synthesis?.source;
            const report =
              source?.mode === 'lane_report'
                ? tick.lanes?.find((lane) => lane.key === source.key)
                : undefined;
            const finding = typeof report?.finding === 'string' ? report.finding : '';
            const nextStep = typeof report?.next_step === 'string' ? report.next_step : '';
            return (
              <article key={tick.tick} className="min-w-0">
                <div className="mb-3 flex items-center gap-2 text-xs text-lz-ink-2">
                  <Bot size={16} />
                  <span>Agent · run {tick.tick}</span>
                  {tick.synthesis?.source?.mode === 'lane_report' && <span>Worker report</span>}
                  <span className="ml-auto">
                    {tick.outcome ??
                      (active && tick.tick === model.tick ? 'In progress' : 'Interrupted')}
                  </span>
                </div>
                {tick.notes?.map((note, index) => (
                  <p key={index} className="mb-3 rounded-lg bg-lz-surface-2 p-3 text-sm">
                    {note}
                  </p>
                ))}
                <MarkdownContent
                  content={
                    finding ||
                    tick.synthesis?.handoff ||
                    tick.synthesis?.log_line ||
                    tick.summary ||
                    'No summary was recorded for this run.'
                  }
                />
                {finding && nextStep && <MarkdownContent content={nextStep} />}
                {finding && tick.synthesis?.handoff && (
                  <ActivityDisclosure label="Evidence and full report">
                    <div className="px-3 py-2">
                      <MarkdownContent content={tick.synthesis.handoff} />
                    </div>
                  </ActivityDisclosure>
                )}
                {!finding && !!tick.synthesis?.facts.length && (
                  <ActivityDisclosure label={`${tick.synthesis.facts.length} recorded findings`}>
                    <ul className="space-y-2 px-3 py-2 text-sm">
                      {tick.synthesis.facts.map((fact, index) => (
                        <li key={index}>{fact}</li>
                      ))}
                    </ul>
                  </ActivityDisclosure>
                )}
                {!!tick.synthesis?.pending.length && (
                  <ActivityDisclosure label="Next steps">
                    <ul className="space-y-2 px-3 py-2 text-sm">
                      {tick.synthesis.pending.map((item, index) => (
                        <li key={index}>{item}</li>
                      ))}
                    </ul>
                  </ActivityDisclosure>
                )}
              </article>
            );
          })}
        </div>
      ) : null}
    </Panel>
  );
}
