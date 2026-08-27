import { useEffect, useRef, useState } from 'react';
import { X, Send, Loader2, Check, Sparkles, Pencil, ChevronDown } from 'lucide-react';
import { toast } from 'react-toastify';
import { LeanZero } from '../icons';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '../ui/dropdown-menu';
import { useFleet } from './useFleet';
import type { Recipe } from '../../recipe';
import { saveRecipe } from '../../recipe/recipe_management';
import { CHIP_RADIUS, SWARM_STATUS } from './formationVisualState';

/**
 * Goose Local Edition — build a recipe by TALKING TO THE FLEET. A warm local model (LM Studio, the same
 * nodes that power the swarm) interviews the user a couple of turns, then drafts a recipe the user reviews,
 * edits, and saves. This is the "work with the swarm to build it" path, distinct from the by-hand form and
 * from the build orchestrator (which produces apps, not recipes, and cannot hold a conversation).
 *
 * The model is driven directly over LM Studio's OpenAI-compatible endpoint on 127.0.0.1 (the app CSP allows
 * that origin; `localhost` is blocked). Weak local models don't always follow a protocol, so there is always
 * a "Draft the recipe now" escape hatch that forces the JSON, and the parsed draft is fully editable.
 */

const AZURE = SWARM_STATUS.action;
const CHAT_URL = 'http://127.0.0.1:1234/v1/chat/completions';

// The conversation is an INTERVIEW only — the model asks questions and never writes the recipe or any
// JSON. The recipe itself is produced by a separate, schema-constrained call (draftRecipe) so a weak local
// model can't emit a truncated / malformed code block into the chat.
const SYSTEM_PROMPT = `You are helping a user create a "recipe" for an autonomous agent — a reusable instruction set the agent runs every time its loop fires. Your only job right now is to INTERVIEW them.
- Ask ONE short, friendly clarifying question at a time (1-2 sentences). Cover, over a few turns: what the agent should do each run, the specific target or scope, and what a good result looks like.
- Do NOT write the recipe yourself, and do NOT output JSON, code blocks, or backticks.
- Once you have enough (usually after 2-3 answers), reply with one short sentence telling the user to click the "Draft the recipe now" button below.`;

const DRAFT_SYSTEM = `From the conversation, produce a recipe for the autonomous agent. Return a title (a short name, 3-6 words), a one-sentence description, and instructions: specific, multi-step, imperative directions the agent follows on every run — concrete and actionable, referencing the details the user gave.`;

// LM Studio (OpenAI-compatible) honours response_format json_schema, forcing valid structured output.
const RECIPE_SCHEMA = {
  type: 'json_schema',
  json_schema: {
    name: 'recipe',
    strict: true,
    schema: {
      type: 'object',
      additionalProperties: false,
      properties: {
        title: { type: 'string' },
        description: { type: 'string' },
        instructions: { type: 'string' },
      },
      required: ['title', 'description', 'instructions'],
    },
  },
};

type ChatRole = 'user' | 'assistant';
interface ChatMsg {
  role: ChatRole;
  content: string;
}

interface Draft {
  title: string;
  description: string;
  instructions: string;
}

/** The visible text of an interview reply. The model is told not to emit code, but strip any stray fence
 *  defensively so the chat never shows raw ```json to the user. */
function visibleText(text: string): string {
  const stripped = text.replace(/```(?:json)?\s*[\s\S]*?(```|$)/gi, '').trim();
  return stripped || 'Tell me a bit more, or click “Draft the recipe now”.';
}

export function RecipeChatWizard({
  isOpen,
  onClose,
  onSaved,
}: {
  isOpen: boolean;
  onClose: () => void;
  onSaved: (recipe: Recipe) => void;
}) {
  const fleet = useFleet();
  const [picked, setPicked] = useState<string | null>(null);
  const autoModel = fleet.models.find((m) => /coder/i.test(m)) ?? fleet.models[0] ?? null;
  // Use the user's pick if it's still loaded, else fall back to the auto-chosen coder model.
  const model = (picked && fleet.models.includes(picked) ? picked : null) ?? autoModel;

  const [messages, setMessages] = useState<ChatMsg[]>([]);
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  // Bumped on every open/close transition; an in-flight request captures the current value and bails out
  // of its setState if the generation changed — so a late reply can't repopulate a closed/reopened chat.
  const genRef = useRef(0);
  const prevOpen = useRef(false);

  useEffect(() => {
    if (isOpen && !prevOpen.current) {
      // Fresh open: seed the greeting and clear everything (including busy/saving, in case a prior request
      // was still in flight when the modal was last closed).
      genRef.current++;
      setMessages([
        {
          role: 'assistant',
          content:
            "Tell me what you'd like this agent to do each time it runs, and I'll shape it into a recipe. What's the task?",
        },
      ]);
      setInput('');
      setDraft(null);
      setError(null);
      setBusy(false);
      setSaving(false);
    } else if (!isOpen && prevOpen.current) {
      genRef.current++;
      setBusy(false);
      setSaving(false);
      setMessages([]);
      setInput('');
      setDraft(null);
      setError(null);
    }
    prevOpen.current = isOpen;
  }, [isOpen]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages, busy, draft]);

  if (!isOpen) return null;

  // One chat completion. `format` optionally forces structured JSON output (used to draft the recipe).
  const complete = async (system: string, history: ChatMsg[], format?: unknown): Promise<string> => {
    if (!model) throw new Error('no-model');
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 120_000);
    try {
      const res = await fetch(CHAT_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          model,
          messages: [{ role: 'system', content: system }, ...history],
          temperature: format ? 0.3 : 0.5,
          max_tokens: 1200,
          stream: false,
          ...(format ? { response_format: format } : {}),
        }),
        signal: controller.signal,
      });
      if (!res.ok) throw new Error(`fleet returned ${res.status}`);
      const data = (await res.json()) as { choices?: Array<{ message?: { content?: string } }> };
      const reply = data.choices?.[0]?.message?.content ?? '';
      if (!reply.trim()) throw new Error('the fleet returned an empty reply');
      return reply;
    } finally {
      clearTimeout(timer);
    }
  };

  const fleetError = (e: unknown): string => {
    const msg =
      e instanceof Error && e.name === 'AbortError'
        ? 'the fleet took too long (is it busy building? free it up and retry)'
        : e instanceof Error && e.message === 'no-model'
          ? 'no fleet model is loaded — start LM Studio and load a model'
          : e instanceof Error
            ? e.message
            : String(e);
    return `Couldn’t reach the fleet — ${msg}.`;
  };

  // Interview turn: the model asks the next question. It never writes the recipe here.
  const ask = async (history: ChatMsg[]): Promise<void> => {
    const gen = genRef.current;
    setBusy(true);
    setError(null);
    try {
      const reply = await complete(SYSTEM_PROMPT, history);
      if (genRef.current !== gen) return;
      setMessages((prev) => [...prev, { role: 'assistant', content: visibleText(reply) }]);
    } catch (e) {
      if (genRef.current !== gen) return;
      setError(fleetError(e));
    } finally {
      if (genRef.current === gen) setBusy(false);
    }
  };

  const send = () => {
    const text = input.trim();
    if (!text || busy) return;
    const next = [...messages, { role: 'user' as const, content: text }];
    setMessages(next);
    setInput('');
    void ask(next);
  };

  // Draft: a schema-constrained call that ALWAYS yields a valid recipe — no truncated / malformed blocks.
  const draftNow = async () => {
    if (busy) return;
    const gen = genRef.current;
    setBusy(true);
    setError(null);
    try {
      const reply = await complete(DRAFT_SYSTEM, messages, RECIPE_SCHEMA);
      if (genRef.current !== gen) return;
      const o = JSON.parse(reply) as Partial<Draft>;
      const title = (o.title ?? '').trim();
      const instructions = (o.instructions ?? '').trim();
      if (!title || !instructions) {
        throw new Error('the fleet returned an incomplete recipe — add a bit more detail and retry');
      }
      setDraft({ title, description: (o.description ?? '').trim() || title, instructions });
      setMessages((prev) => [
        ...prev,
        { role: 'assistant', content: 'Drafted your recipe — review and edit it below, then save.' },
      ]);
    } catch (e) {
      if (genRef.current !== gen) return;
      setError(fleetError(e));
    } finally {
      if (genRef.current === gen) setBusy(false);
    }
  };

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    setError(null);
    try {
      const recipe: Recipe = {
        version: '1.0.0',
        title: draft.title.trim(),
        description: draft.description.trim() || draft.title.trim(),
        instructions: draft.instructions.trim(),
      };
      await saveRecipe(recipe, null);
      toast.success(`Recipe "${recipe.title}" saved`);
      onSaved(recipe);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const inputClass =
    'w-full bg-background-primary border border-border-primary px-2.5 py-1.5 text-sm text-text-primary focus:border-text-secondary outline-none';

  return (
    <div className="fixed inset-0 z-[75] flex items-center justify-center bg-black/50 p-4">
      <div
        className="bg-background-primary border border-border-primary shadow-lg w-[620px] max-h-[88vh] flex flex-col"
        style={{ borderRadius: CHIP_RADIUS }}
        data-testid="recipe-chat-wizard"
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-border-primary">
          <div className="flex items-center gap-2">
            <span className="flex items-center justify-center h-6 w-6" style={{ backgroundColor: AZURE }}>
              <LeanZero className="h-4 w-4 text-white" />
            </span>
            <h3 className="text-sm font-semibold text-text-primary">Build a recipe with the fleet</h3>
          </div>
          <div className="flex items-center gap-2">
            {fleet.online && fleet.models.length > 0 ? (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    className="hidden sm:flex items-center gap-1 text-[11px] font-mono px-1.5 py-0.5 border"
                    style={{ borderRadius: CHIP_RADIUS, borderColor: AZURE, color: AZURE }}
                    title={model ? `${model} — click to switch node` : 'pick a node'}
                  >
                    <span className="h-1.5 w-1.5" style={{ backgroundColor: SWARM_STATUS.done, borderRadius: CHIP_RADIUS }} />
                    {model ? model.split('-')[0] : 'model'}
                    <ChevronDown className="h-3 w-3" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  {fleet.models.map((m) => (
                    <DropdownMenuItem key={m} onClick={() => setPicked(m)} className="text-xs font-mono">
                      {m === model && <Check className="h-3 w-3 mr-1 shrink-0" style={{ color: AZURE }} />}
                      {m}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            ) : (
              <span
                className="hidden sm:flex items-center gap-1 text-[11px] font-mono px-1.5 py-0.5 border border-border-primary"
                style={{ borderRadius: CHIP_RADIUS }}
              >
                <span className="h-1.5 w-1.5" style={{ backgroundColor: SWARM_STATUS.error, borderRadius: CHIP_RADIUS }} />
                offline
              </span>
            )}
            <button onClick={onClose} className="text-text-secondary hover:text-text-primary" aria-label="Close">
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        {/* Conversation */}
        <div ref={scrollRef} className="px-4 py-3 overflow-y-auto space-y-3 flex-1 min-h-[220px]">
          {messages.map((m, i) => (
            <div key={i} className={`flex ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}>
              <div
                className={`max-w-[80%] px-3 py-2 text-sm whitespace-pre-wrap break-words ${
                  m.role === 'assistant' ? 'bg-background-secondary border border-border-primary' : ''
                }`}
                style={
                  m.role === 'user'
                    ? { backgroundColor: AZURE, color: '#fff', borderRadius: CHIP_RADIUS }
                    : { borderRadius: CHIP_RADIUS }
                }
              >
                {m.role === 'assistant' && (
                  <span className="flex items-center gap-1 text-[10px] uppercase tracking-wide text-text-secondary mb-1">
                    <Sparkles className="h-3 w-3" /> fleet
                  </span>
                )}
                <span className={m.role === 'assistant' ? 'text-text-primary' : ''}>{m.content}</span>
              </div>
            </div>
          ))}
          {busy && (
            <div className="flex justify-start">
              <div className="flex items-center gap-2 text-xs text-text-secondary px-3 py-2">
                <Loader2 className="h-3.5 w-3.5 animate-spin" /> the fleet is thinking…
              </div>
            </div>
          )}
        </div>

        {/* Draft review card */}
        {draft && (
          <div className="mx-4 mb-2 border border-border-primary" style={{ borderRadius: CHIP_RADIUS }}>
            <div className="px-3 py-1.5 text-xs font-semibold flex items-center gap-1.5 text-white" style={{ backgroundColor: AZURE }}>
              <Pencil className="h-3.5 w-3.5" /> Draft recipe — review &amp; edit before saving
            </div>
            <div className="p-3 space-y-2">
              <input
                value={draft.title}
                onChange={(e) => setDraft({ ...draft, title: e.target.value })}
                placeholder="Title"
                className={inputClass}
                style={{ borderRadius: CHIP_RADIUS }}
              />
              <input
                value={draft.description}
                onChange={(e) => setDraft({ ...draft, description: e.target.value })}
                placeholder="One-line description"
                className={inputClass}
                style={{ borderRadius: CHIP_RADIUS }}
              />
              <textarea
                value={draft.instructions}
                onChange={(e) => setDraft({ ...draft, instructions: e.target.value })}
                rows={5}
                placeholder="Instructions the agent follows every run"
                className={inputClass}
                style={{ borderRadius: CHIP_RADIUS, resize: 'vertical' }}
              />
            </div>
          </div>
        )}

        {error && (
          <div className="mx-4 mb-2 text-xs px-3 py-2 border" style={{ borderColor: SWARM_STATUS.error, color: SWARM_STATUS.error, borderRadius: CHIP_RADIUS }}>
            {error}
          </div>
        )}

        {/* Input + actions */}
        <div className="px-4 py-3 border-t border-border-primary space-y-2">
          <div className="flex items-end gap-2">
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                  e.preventDefault();
                  send();
                }
              }}
              rows={1}
              placeholder="Answer the fleet…  (Enter to send)"
              className={`${inputClass} flex-1`}
              style={{ borderRadius: CHIP_RADIUS, resize: 'none' }}
            />
            <button
              onClick={send}
              disabled={busy || !input.trim()}
              className="flex items-center gap-1 text-xs font-semibold px-3 py-2 text-white transition-opacity hover:opacity-90 disabled:opacity-50"
              style={{ backgroundColor: AZURE, borderRadius: CHIP_RADIUS }}
            >
              <Send className="h-3.5 w-3.5" /> Send
            </button>
          </div>
          <div className="flex items-center justify-between">
            <button
              onClick={() => void draftNow()}
              disabled={busy || messages.length < 2}
              className="text-xs px-2.5 py-1 border border-border-primary text-text-primary hover:border-text-secondary transition-colors disabled:opacity-50 flex items-center gap-1"
              style={{ borderRadius: CHIP_RADIUS }}
            >
              <Sparkles className="h-3.5 w-3.5" /> Draft the recipe now
            </button>
            <button
              onClick={() => void save()}
              disabled={!draft || saving}
              className="flex items-center gap-1.5 text-xs font-semibold px-3 py-1.5 text-white transition-opacity hover:opacity-90 disabled:opacity-40"
              style={{ backgroundColor: draft ? SWARM_STATUS.done : '#6b7280', borderRadius: CHIP_RADIUS }}
            >
              {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Check className="h-3.5 w-3.5" />}
              Save recipe
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export default RecipeChatWizard;
