import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { X, Send, Loader2, Check, Sparkles, Pencil, ChevronDown } from 'lucide-react';
import { toast } from 'react-toastify';
import { LeanZero } from '../icons';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '../ui/dropdown-menu';
import { useFleet } from './useFleet';
import { useLmStudioFleetVisible } from '../../hooks/useLmStudioFleetVisible';
import { useMlxEngineStatusPoll } from '../leanzero-swarm/useMlxEngineStatus';
import { useLatestMlxDistributedStatus } from '../noNodeNotice/mlxMount';
import { mlxDistributedStatus } from '../../acp/mlx-distributed';
import {
  latestMlxRemoteSingleStatus,
  mlxRemoteSingleStatus,
  subscribeMlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import { recipeChatTarget } from './recipeChatTarget';
import type { Recipe } from '../../recipe';
import { saveRecipe } from '../../recipe/recipe_management';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import {
  Button,
  DISABLED,
  FOCUS,
  MOTION,
  RADIUS,
  StatusDot,
  SURFACE,
  TONE_FILL,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';

/**
 * Goose Local Edition — build a recipe by TALKING TO THE FLEET. The model goose serves chat with (the
 * split across the Macs, a linked Mac's engine through the route, or this Mac's engine — the one
 * `mlxEngineServing` rule the chip reads, `recipeChatTarget`) interviews the user a couple of turns,
 * then drafts a recipe the user reviews, edits, and saves. It is the default pick (Q-7: the wizard used
 * to see only this Mac's single engine and LM Studio, and read "no fleet model is served" while the
 * split answered every chat). LM Studio's models are offered beside it only while the legacy LM Studio
 * fleet is shown; picking one sends the interview to LM Studio's configured endpoint.
 *
 * Each call is an OpenAI-compatible chat completion at the picked engine's base URL. The POST is
 * non-streaming and goes through MAIN (`window.electron.fleetChat` → IPC `fleet-chat`): the renderer's CSP is the
 * intersection of index.html's static meta and main's header, which blocks `localhost` and any LAN host
 * from here no matter what the header adds (gate 8, 2026-09-02). Weak local models don't always follow a
 * protocol, so there is always a "Draft the recipe now" escape hatch that forces the JSON, and the parsed
 * draft is fully editable.
 */

/** The Studio field recipe every input and textarea in this wizard shares. */
const FIELD = cx(
  'w-full border border-lz-border-strong bg-lz-surface px-2.5 py-1.5 text-lz-body text-lz-ink placeholder:text-lz-ink-3',
  RADIUS.control,
  FOCUS
);

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
  // LEGACY surface: LM Studio model discovery runs only when 'showLmStudioFleet' is on (default
  // off). Off, the wizard shows its honest offline path — it cannot draft without a served model.
  const fleet = useFleet(5000, undefined, useLmStudioFleetVisible());
  // The engine goose serves chat with — the split, a route to a linked Mac, or this Mac's single
  // engine — read the way the composer's chip reads it (Q-7). The split and the route publish into
  // shared stores; one read on open fills them when no other surface has yet.
  const { status: mlx } = useMlxEngineStatusPoll(isOpen);
  const distributed = useLatestMlxDistributedStatus();
  const remote = useSyncExternalStore(subscribeMlxRemoteSingleStatus, latestMlxRemoteSingleStatus);
  useEffect(() => {
    if (!isOpen) return;
    mlxDistributedStatus().catch(() => undefined);
    mlxRemoteSingleStatus().catch(() => undefined);
  }, [isOpen]);
  const served = recipeChatTarget(mlx, distributed, remote);
  const mlxModel = served?.model ?? null;
  const models = mlxModel
    ? [mlxModel, ...fleet.models.filter((m) => m !== mlxModel)]
    : fleet.models;
  const [picked, setPicked] = useState<string | null>(null);
  // Use the user's pick while it is still served, else the model chat is served by, else (legacy
  // LM Studio fleet only) a coder model.
  const autoModel = mlxModel ?? models.find((m) => /coder/i.test(m)) ?? models[0] ?? null;
  const model = (picked && models.includes(picked) ? picked : null) ?? autoModel;
  // The host that serves the picked model: that engine's own base URL for the chat model, LM
  // Studio's configured endpoint for its models (fleetChat takes the origin of either).
  const chatEndpoint =
    model != null && model === mlxModel && served ? served.baseUrl : fleet.endpoint;
  const online = models.length > 0 && (model === mlxModel || fleet.online);

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
  const complete = async (
    system: string,
    history: ChatMsg[],
    format?: unknown
  ): Promise<string> => {
    if (!model) throw new Error('no-model');
    const r = await window.electron.fleetChat(chatEndpoint, {
      model,
      messages: [{ role: 'system', content: system }, ...history],
      temperature: format ? 0.3 : 0.5,
      max_tokens: 1200,
      stream: false,
      ...(format ? { response_format: format } : {}),
    });
    if (!r.ok) {
      if (r.error === 'timeout') {
        // Same shape the in-renderer AbortController produced, so fleetError's message holds.
        const e = new Error('timeout');
        e.name = 'AbortError';
        throw e;
      }
      if (r.error === 'http') throw new Error(`fleet returned ${r.status}`);
      throw new Error(`${r.error} at ${r.url}: ${r.detail}`);
    }
    const data = r.body as { choices?: Array<{ message?: { content?: string } }> };
    const reply = data?.choices?.[0]?.message?.content ?? '';
    if (!reply.trim()) throw new Error('the fleet returned an empty reply');
    return reply;
  };

  const fleetError = (e: unknown): string => {
    const msg =
      e instanceof Error && e.name === 'AbortError'
        ? 'the fleet took too long (is it busy building? free it up and retry)'
        : e instanceof Error && e.message === 'no-model'
          ? 'no fleet model is served — start one in Providers › LeanZero MLX › Run it'
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
        throw new Error(
          'the fleet returned an incomplete recipe — add a bit more detail and retry'
        );
      }
      setDraft({ title, description: (o.description ?? '').trim() || title, instructions });
      setMessages((prev) => [
        ...prev,
        {
          role: 'assistant',
          content: 'Drafted your recipe — review and edit it below, then save.',
        },
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

  return (
    <OverlayDialog
      open
      onClose={onClose}
      layerClassName="z-[75]"
      panelClassName={cx('flex w-[620px] max-h-[88vh] flex-col', SURFACE.overlay)}
      data-testid="recipe-chat-wizard"
    >
      <div className="flex items-center justify-between border-b border-lz-border px-4 py-3">
        <div className="flex items-center gap-2">
          <span
            className={cx(
              'flex h-6 w-6 items-center justify-center',
              RADIUS.control,
              TONE_FILL.accent
            )}
          >
            <LeanZero className="h-4 w-4 text-white" />
          </span>
          <OverlayDialogTitle className={TYPE.h2}>Build a recipe with the fleet</OverlayDialogTitle>
        </div>
        <div className="flex items-center gap-2">
          {online ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  className={cx(
                    'hidden sm:flex items-center gap-1 border border-lz-accent px-1.5 py-0.5 font-mono text-lz-mono text-lz-accent',
                    RADIUS.control,
                    MOTION,
                    FOCUS
                  )}
                  title={model ? `${model} — click to switch node` : 'pick a node'}
                >
                  <StatusDot tone="ok" label="fleet online" size={8} />
                  {model ? model.split('-')[0] : 'model'}
                  <ChevronDown className="h-3 w-3" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {models.map((m) => (
                  <DropdownMenuItem
                    key={m}
                    onClick={() => setPicked(m)}
                    className="text-xs font-mono"
                  >
                    {m === model && (
                      <Check className={cx('h-3 w-3 mr-1 shrink-0', TONE_TEXT.accent)} />
                    )}
                    {m}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : (
            <span
              className={cx(
                'hidden sm:flex items-center gap-1 border border-lz-border-strong px-1.5 py-0.5 font-mono text-lz-mono text-lz-ink-3',
                RADIUS.control
              )}
            >
              <StatusDot tone="err" label="fleet offline" size={8} />
              offline
            </span>
          )}
          <button
            onClick={onClose}
            className={cx('text-lz-ink-3 hover:text-lz-ink', MOTION, FOCUS)}
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      </div>

      {/* Conversation */}
      <div ref={scrollRef} className="min-h-[220px] flex-1 space-y-3 overflow-y-auto px-4 py-3">
        {messages.map((m, i) => (
          <div key={i} className={cx('flex', m.role === 'user' ? 'justify-end' : 'justify-start')}>
            <div
              className={cx(
                'max-w-[80%] whitespace-pre-wrap break-words px-3 py-2 text-lz-body',
                RADIUS.control,
                m.role === 'user'
                  ? TONE_FILL.accent
                  : 'border border-lz-border bg-lz-surface-2 text-lz-ink'
              )}
            >
              {m.role === 'assistant' && (
                <span className="mb-1 flex items-center gap-1 text-lz-zone uppercase text-lz-ink-3">
                  <Sparkles className="h-3 w-3" /> fleet
                </span>
              )}
              <span>{m.content}</span>
            </div>
          </div>
        ))}
        {busy && (
          <div className="flex justify-start">
            <div className="flex items-center gap-2 px-3 py-2 text-lz-meta text-lz-ink-3">
              <Loader2 className="h-3.5 w-3.5 animate-spin" /> the fleet is thinking…
            </div>
          </div>
        )}
      </div>

      {/* Draft review card */}
      {draft && (
        <div className={cx('mx-4 mb-2 overflow-hidden border border-lz-border', RADIUS.control)}>
          <div
            className={cx(
              'flex items-center gap-1.5 px-3 py-1.5 text-lz-meta',
              WEIGHT.semibold,
              TONE_FILL.accent
            )}
          >
            <Pencil className="h-3.5 w-3.5" /> Draft recipe — review &amp; edit before saving
          </div>
          <div className="space-y-2 p-3">
            <input
              value={draft.title}
              onChange={(e) => setDraft({ ...draft, title: e.target.value })}
              placeholder="Title"
              className={FIELD}
            />
            <input
              value={draft.description}
              onChange={(e) => setDraft({ ...draft, description: e.target.value })}
              placeholder="One-line description"
              className={FIELD}
            />
            <textarea
              value={draft.instructions}
              onChange={(e) => setDraft({ ...draft, instructions: e.target.value })}
              rows={5}
              placeholder="Instructions the agent follows every run"
              className={cx(FIELD, 'resize-y')}
            />
          </div>
        </div>
      )}

      {error && (
        <div
          className={cx(
            'mx-4 mb-2 border border-lz-err px-3 py-2 text-lz-meta',
            TONE_TEXT.err,
            RADIUS.control
          )}
        >
          {error}
        </div>
      )}

      {/* Input + actions */}
      <div className="space-y-2 border-t border-lz-border px-4 py-3">
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
            className={cx(FIELD, 'flex-1 resize-none')}
          />
          <Button variant="primary" onClick={send} disabled={busy || !input.trim()} icon={<Send />}>
            Send
          </Button>
        </div>
        <div className="flex items-center justify-between">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => void draftNow()}
            disabled={busy || messages.length < 2}
            icon={<Sparkles />}
          >
            Draft the recipe now
          </Button>
          {/* The ok fill once a draft exists (a status-tone action, like "Run agent now"); the solid
                disabled state before that — never a hand-written grey, never an opacity. */}
          <button
            onClick={() => void save()}
            disabled={!draft || saving}
            className={cx(
              'flex items-center gap-1.5 border border-lz-ok-solid px-3 py-1.5 text-lz-meta',
              WEIGHT.semibold,
              TONE_FILL.ok,
              RADIUS.control,
              MOTION,
              FOCUS,
              DISABLED
            )}
          >
            {saving ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Check className="h-3.5 w-3.5" />
            )}
            Save recipe
          </button>
        </div>
      </div>
    </OverlayDialog>
  );
}

export default RecipeChatWizard;
