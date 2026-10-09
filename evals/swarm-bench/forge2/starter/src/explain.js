import { chat, list } from '@forge/llm';

const TOOL = {
  type: 'function',
  function: {
    name: 'report_scope',
    description: 'Report why the sprint scope changed: a short plain-language summary and the ids of the changes that drove it.',
    parameters: {
      type: 'object',
      properties: {
        summary: { type: 'string', description: 'Two or three sentences explaining the scope creep in words. Do not write any digits or numbers.' },
        changeIds: { type: 'array', items: { type: 'string' }, description: 'The changeId values of the changes that explain the creep most.' },
      },
      required: ['summary', 'changeIds'],
    },
  },
};

const SYSTEM = [
  'You explain scope creep in one Jira sprint to an agile coach.',
  'You receive the team totals and the changes the reader is allowed to see.',
  'Answer only by calling report_scope. In summary, use words only: no digits, no numbers, no percentages,',
  'because the app shows the exact numbers next to your text. In changeIds, list only changeId values from the input.',
].join(' ');

// The model is whichever claude model the platform reports active right now, never a hard-coded name.
async function activeModel() {
  const { models } = await list();
  const active = (models ?? []).filter((m) => m.status === 'active').map((m) => m.model);
  return active.find((m) => /sonnet/i.test(m)) ?? active[0] ?? null;
}

function ledgerSentence(view) {
  const { committed, added, removed, creep } = view.text;
  return creep === '—'
    ? `Nothing was committed when ${view.sprint.name} started; ${added} points entered and ${removed} points left it since.`
    : `${view.sprint.name} grew by ${creep}: ${added} points entered after the start against ${committed} committed, and ${removed} points left it.`;
}

// view = personView(): only what this viewer may see is ever sent to the model.
export async function explainSprint(view) {
  let response;
  let model;
  try {
    model = await activeModel();
    if (!model) return { ok: false, error: 'No Forge LLM model is active right now.' };
    response = await chat({
      model,
      messages: [
        { role: 'system', content: [{ type: 'text', text: SYSTEM }] },
        {
          role: 'user',
          content: [
            {
              type: 'text',
              text: JSON.stringify({
                sprint: view.sprint.name,
                totals: view.text,
                changes: view.changes.map((c) => ({ changeId: c.changeId, issueKey: c.issueKey, kind: c.kind, points: c.points, by: c.by, at: c.at })),
              }),
            },
          ],
        },
      ],
      tools: [TOOL],
      tool_choice: { type: 'function', function: { name: 'report_scope' } },
      // No temperature/top_p: current Claude models refuse sampling parameters (measured on forge-dev:
      // 400 "does not support the temperature and top_p sampling parameters").
      max_completion_tokens: 600,
    });
  } catch (e) {
    console.error(`explain: Forge LLM call failed (${model ?? 'no model'}): ${e.message}`);
    return { ok: false, error: `Forge LLM could not explain this sprint: ${e.message}` };
  }

  const call = (response?.choices ?? []).flatMap((c) => c.message?.tool_calls ?? []).find((t) => t.function?.name === 'report_scope');
  if (!call) return { ok: false, error: 'The model declined to explain this sprint.' };
  const args = call.function.arguments;
  const valid =
    args !== null &&
    typeof args === 'object' &&
    typeof args.summary === 'string' &&
    Array.isArray(args.changeIds) &&
    args.changeIds.every((id) => typeof id === 'string');
  if (!valid) return { ok: false, error: 'The model answered in an unexpected shape.' };

  const visible = new Map(view.changes.map((c) => [c.changeId, c]));
  const ids = [...new Set(args.changeIds)].filter((id) => visible.has(id));
  const summary = /\d/.test(args.summary) || !args.summary.trim() ? ledgerSentence(view) : args.summary.trim();
  return {
    ok: true,
    model,
    summary,
    changes: ids.map((id) => ({ changeId: id, issueKey: visible.get(id).issueKey, kind: visible.get(id).kind })),
  };
}
