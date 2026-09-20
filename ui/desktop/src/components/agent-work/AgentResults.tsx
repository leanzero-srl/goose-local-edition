import { Bot } from 'lucide-react';
import MarkdownContent from '../MarkdownContent';
import { ActivityDisclosure } from '../activity/ActivityDisclosure';
import { EmptyState, Panel } from '../lz';
import type { DeskModel } from './agentWorkModel';

export function AgentResults({ model }: { model: DeskModel }) {
  const ticks = [...model.ticks].sort((a, b) => b.tick - a.tick);
  return (
    <Panel title="Conversation & results">
      {ticks.length === 0 ? (
        <EmptyState
          icon={<Bot />}
          title="Ready for the first assignment"
          body="Run once to try the agent. Its findings and handoff appear here; live tool activity appears below."
        />
      ) : (
        <div className="space-y-6">
          {ticks.map((tick) => (
            <article key={tick.tick} className="min-w-0">
              <div className="mb-3 flex items-center gap-2 text-xs text-lz-ink-2">
                <Bot size={16} />
                <span>Agent · run {tick.tick}</span>
                <span className="ml-auto">{tick.outcome ?? 'In progress'}</span>
              </div>
              {tick.notes?.map((note, index) => (
                <p key={index} className="mb-3 rounded-lg bg-lz-surface-2 p-3 text-sm">
                  {note}
                </p>
              ))}
              <MarkdownContent
                content={
                  tick.synthesis?.handoff ||
                  tick.synthesis?.log_line ||
                  tick.summary ||
                  'No summary was recorded for this run.'
                }
              />
              {!!tick.synthesis?.facts.length && (
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
          ))}
        </div>
      )}
    </Panel>
  );
}
