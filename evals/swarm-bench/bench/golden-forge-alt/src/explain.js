import { list, stream } from '@forge/llm';
import { sprintReport } from './report';
import { fmtPoints } from './model';

const TOOL_NAME = 'report_scope';
const REPORT_TOOL = {
  type: 'function',
  function: {
    name: TOOL_NAME,
    description: 'Report why this sprint changed scope after it started, and which listed changes drove it.',
    parameters: {
      type: 'object',
      properties: {
        summary: { type: 'string', description: 'Two or three plain sentences for an agile coach. Do not write any numbers or digits.' },
        changeIds: { type: 'array', items: { type: 'string' }, description: 'The changeId values, copied exactly from the list, of the changes that drove the scope change.' },
      },
      required: ['summary', 'changeIds'],
      additionalProperties: false,
    },
  },
};

class ExplainFailure extends Error {
  constructor(kind, message) {
    super(message);
    this.kind = kind;
  }
}

// Any active model the llm module's family offers; the newest name sorts last.
async function activeModel() {
  const { models = [] } = await list();
  const active = models.filter((m) => m && m.status === 'active' && typeof m.model === 'string').map((m) => m.model);
  if (!active.length) throw new ExplainFailure('error', 'Forge LLM lists no active model.');
  return active.sort()[active.length - 1];
}

// Only what the viewer may see: team totals (the same for everyone) and their visible changes.
function promptFor(report) {
  const s = report.summary;
  const lines = report.changes.map((c) => `- changeId ${c.changeId}: ${c.issueKey} ${c.kind} at ${c.at} by ${c.by}, ${fmtPoints(c.points)} points`);
  return [
    {
      role: 'system',
      content: [{ type: 'text', text: 'You explain sprint scope change to agile coaches. Answer only by calling the report_scope tool. Never put numbers or digits in the summary; refer to changes only by the changeId values given.' }],
    },
    {
      role: 'user',
      content: [
        {
          type: 'text',
          text: [
            `Sprint "${report.sprint.name}" started ${report.sprint.startDate}.`,
            `Committed ${fmtPoints(s.committed)} points; added ${fmtPoints(s.added)}; removed ${fmtPoints(s.removed)}; scope creep ${s.creepText}.`,
            report.changes.length ? 'Changes after the start:' : 'No changes after the start are visible.',
            ...lines,
          ].join('\n'),
        },
      ],
    },
  ];
}

// Streamed tool calls are folded per call: a name, and arguments that arrive either as an
// object (merged) or as string fragments (concatenated, parsed once at the end).
async function streamedToolCalls(response) {
  const calls = new Map();
  try {
    for await (const chunk of response) {
      for (const choice of (chunk && chunk.choices) || []) {
        for (const tc of (choice.message && choice.message.tool_calls) || []) {
          const slot = tc.index ?? tc.id ?? calls.size;
          const call = calls.get(slot) || { name: '', fragments: [], object: null };
          if (tc.function && tc.function.name) call.name = tc.function.name;
          const args = tc.function ? tc.function.arguments : undefined;
          if (typeof args === 'string') {
            const sofar = call.fragments.join('');
            // Some streams resend the whole argument string so far; others send only the delta.
            if (sofar && args.startsWith(sofar)) call.fragments = [args];
            else call.fragments.push(args);
          }
          else if (args && typeof args === 'object') call.object = { ...(call.object || {}), ...args };
          calls.set(slot, call);
        }
      }
    }
  } finally {
    await response.close?.();
  }
  return [...calls.values()];
}

function argumentsOf(call) {
  if (call.object) return call.object;
  if (!call.fragments.length) return null;
  try {
    return JSON.parse(call.fragments.join(''));
  } catch {
    throw new ExplainFailure('malformed', 'The model returned report_scope arguments that are not JSON.');
  }
}

// Strict reading of the one allowed tool call: anything else is a refusal or malformed.
function readReport(calls) {
  if (!calls.length) throw new ExplainFailure('refused', 'The model answered without calling report_scope.');
  const reports = calls.filter((c) => c.name === TOOL_NAME);
  if (reports.length !== 1 || calls.length !== 1) throw new ExplainFailure('malformed', `Expected one ${TOOL_NAME} call, got ${calls.map((c) => c.name || '?').join(', ')}.`);
  const args = argumentsOf(reports[0]);
  if (!args || typeof args !== 'object' || Array.isArray(args)) throw new ExplainFailure('malformed', 'report_scope arguments are not an object.');
  const { summary, changeIds } = args;
  if (typeof summary !== 'string' || !summary.trim()) throw new ExplainFailure('malformed', 'report_scope.summary is not a non-empty string.');
  if (!Array.isArray(changeIds) || changeIds.some((id) => typeof id !== 'string')) throw new ExplainFailure('malformed', 'report_scope.changeIds is not an array of strings.');
  return { summary: summary.trim(), changeIds };
}

function ledgerSentence(report) {
  const s = report.summary;
  const creep = s.creepPercent == null ? 'no creep can be computed because nothing was committed' : `scope creep is ${s.creepText}`;
  return `${report.sprint.name}: ${fmtPoints(s.committed)} points were committed at the start, ${fmtPoints(s.added)} were added and ${fmtPoints(s.removed)} removed since, so ${creep}.`;
}

export async function explainSprint(sprintId, accountId) {
  const report = await sprintReport(sprintId, accountId);
  if (report.error) return { error: 'error', message: report.error };
  if (report.notStarted) return { error: 'error', message: 'The sprint has not started.' };
  try {
    const model = await activeModel();
    const response = await stream({
      model,
      messages: promptFor(report),
      tools: [REPORT_TOOL],
      tool_choice: { type: 'function', function: { name: TOOL_NAME } },
      // No temperature/top_p: the models page says some models reject both.
      max_completion_tokens: 600,
    });
    const { summary, changeIds } = readReport(await streamedToolCalls(response));
    const visible = new Set(report.changes.map((c) => c.changeId));
    const kept = [...new Set(changeIds)].filter((id) => visible.has(id));
    const byId = new Map(report.changes.map((c) => [c.changeId, c]));
    return {
      summary: /\d/.test(summary) ? ledgerSentence(report) : summary,
      summaryFromModel: !/\d/.test(summary),
      changes: kept.map((id) => ({ changeId: id, issueKey: byId.get(id).issueKey, kind: byId.get(id).kind })),
    };
  } catch (err) {
    if (err instanceof ExplainFailure) return { error: err.kind, message: err.message };
    return { error: 'error', message: `Forge LLM failed: ${err && err.message ? err.message : String(err)}` };
  }
}
