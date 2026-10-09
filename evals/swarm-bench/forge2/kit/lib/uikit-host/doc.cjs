'use strict';
// Pure functions over ForgeDoc trees: the JSON tree @forge/react 12.3.0's reconciler sends on every commit as
// callBridge('reconcile', { forgeDoc }) — nodes {type, key, props, children}, text as {type:'String', props:{text}},
// the root {type:'Root'} (research/uikit.md §2.1). The product's host renderer that turns this into Atlaskit is
// Atlassian-internal, so this host never draws pixels: it snapshots the tree, renders it to text, and resolves
// visible labels to the elements they name.

const FN = '[function]';

// The reconciler sends the LIVE root and keeps mutating it (props replaced in prepareUpdate, children spliced,
// text rewritten: research/uikit.md verification #24), so every reconcile is copied at receipt. Values follow the
// bridge's port-rpc rules (global-bridge.js module 906): a function cannot cross (named '[function]'), symbols and
// cycles become undefined, a React element passed as a prop loses $$typeof (kept as {type, props}).
function cloneValue(v, seen) {
  if (typeof v === 'function') return FN;
  if (typeof v === 'symbol' || typeof v === 'bigint') return undefined;
  if (v === null || typeof v !== 'object') return v;
  if (seen.has(v)) return undefined;
  seen.add(v);
  if (v instanceof Date) return v.toISOString();
  if (typeof v.$$typeof === 'symbol') {
    return { type: typeof v.type === 'string' ? v.type : FN, props: cloneValue(v.props ?? {}, seen) };
  }
  if (Array.isArray(v)) return v.map((x) => cloneValue(x, seen));
  const out = {};
  for (const [k, x] of Object.entries(v)) {
    const c = cloneValue(x, seen);
    if (c !== undefined) out[k] = c;
  }
  return out;
}

function snapshot(node) {
  const seen = new WeakSet();
  const walk = (n) => {
    const s = { type: n.type, key: n.key, props: cloneValue(n.props ?? {}, seen), children: (n.children ?? []).map(walk) };
    if (n.forgeReactMajorVersion !== undefined) s.forgeReactMajorVersion = n.forgeReactMajorVersion;
    return s;
  };
  return walk(node);
}

// The live handlers of one commit, by node key: a click or change is delivered to the handler of the LATEST commit,
// never to a closure from an older one (useForm's handlers read formState: research/uikit.md verification #9b).
function handlersOf(node, out = new Map()) {
  const fns = {};
  for (const [k, v] of Object.entries(node.props ?? {})) if (typeof v === 'function') fns[k] = v;
  out.set(node.key, fns);
  for (const c of node.children ?? []) handlersOf(c, out);
  return out;
}

function walk(node, visit, ancestors = []) {
  if (!node) return;
  visit(node, ancestors);
  const next = [...ancestors, node];
  for (const c of node.children ?? []) walk(c, visit, next);
}

// ---- text ------------------------------------------------------------------------------------------------------
// Children joined with '' (a line of inline content), with ' ' (a horizontal layout), or with '\n' (everything else).
const INLINE = new Set(['Text', 'Heading', 'Strong', 'Em', 'Strike', 'Code', 'CodeBlock', 'Link', 'Lozenge', 'Badge', 'Label',
  'ErrorMessage', 'HelperMessage', 'ValidMessage', 'Tooltip', 'ModalTitle', 'ListItem', 'Tag', 'User', 'SectionMessageAction',
  'Button', 'LoadingButton', 'LinkButton', 'Pressable', 'Tab']);
const ROW = new Set(['Inline', 'ButtonGroup', 'TagGroup', 'TabList']);
const BUTTONS = new Set(['Button', 'LoadingButton', 'LinkButton', 'Pressable', 'Link']);
const TEXT_INPUTS = new Set(['Textfield', 'TextArea']);
const CHECKS = new Set(['Toggle', 'Checkbox']);
const INPUTS = new Set([...TEXT_INPUTS, ...CHECKS, 'Select', 'RadioGroup', 'DatePicker', 'TimePicker', 'Range', 'UserPicker']);
// String-valued props a person sees as text on these containers (FormSection/SectionMessage/FormHeader title,
// DynamicTable caption, EmptyState header and description).
const VISIBLE_PROPS = ['title', 'caption', 'header', 'description'];

const squash = (s) => String(s).replace(/\s+/g, ' ').trim();

// What the person sees in an input: a controlled `value` wins, then what the person typed (host state, `typed`),
// then the current `defaultValue` (an untouched uncontrolled input follows it, as a DOM input does until it is dirty).
function inputValue(n, typed) {
  if (n.props.value !== undefined && n.props.value !== null) return n.props.value;
  if (typed && typed.has(n.key)) return typed.get(n.key);
  return n.props.defaultValue;
}
function isChecked(n, typed) {
  if (n.props.isChecked !== undefined) return Boolean(n.props.isChecked);
  if (typed && typed.has(n.key)) return Boolean(typed.get(n.key));
  return Boolean(n.props.defaultChecked);
}
const optionLabel = (o) => (o && typeof o === 'object' ? String(o.label ?? o.value ?? '') : String(o ?? ''));

// ADS DynamicTable sorts its rows host-side by the CELL KEY of the sort column (docs: "Sorting a dynamic table is done
// based on the `key` set on each cell"), with Intl.Collator(undefined, {numeric: true, sensitivity: 'accent'}) and
// numbers before strings (@atlaskit/dynamic-table 19.3.8 hoc/with-sorted-page-rows.js:47-78). The locale is pinned
// to en-US here (research/uikit.md verification #12).
const COLLATOR = new Intl.Collator('en-US', { numeric: true, sensitivity: 'accent' });
function sortRows(rows, headCells, sortKey, sortOrder) {
  const col = headCells.findIndex((c) => c.props.cellKey === sortKey);
  if (col < 0) return rows;
  const keyOf = (r) => r.children[col]?.props.cellKey;
  const mod = sortOrder === 'DESC' ? -1 : 1;
  return [...rows].sort((a, b) => {
    const x = keyOf(a);
    const y = keyOf(b);
    if (x === undefined || y === undefined) return mod;
    if (typeof x === 'number' && typeof y === 'number') return mod * (x - y);
    if (typeof x === 'number') return -1;
    if (typeof y === 'number') return 1;
    return mod * COLLATOR.compare(String(x), String(y));
  });
}
function tableLines(n, typed) {
  const part = (name) => n.children.find((c) => c.type === 'ContentWrapper' && c.props.name === name)?.children ?? [];
  const head = part('head');
  const sortKey = n.props.sortKey ?? n.props.defaultSortKey;
  const rows = sortKey ? sortRows(part('rows'), head, sortKey, n.props.sortOrder ?? n.props.defaultSortOrder) : part('rows');
  const cells = (cs) => cs.map((c) => squash(textOf(c, typed).replace(/\n/g, ' '))).join(' | ');
  const lines = [];
  if (head.length) lines.push(cells(head));
  for (const r of rows) lines.push(cells(r.children));
  if (n.props.isLoading) lines.push('(loading)');
  return lines;
}

function textOf(n, typed = null) {
  if (!n) return '';
  if (n.type === 'String') return String(n.props.text ?? '');
  const kids = (sep) => n.children.map((c) => textOf(c, typed)).filter((s) => s !== '').join(sep);
  const flags = (...fs) => fs.filter(Boolean).map((f) => ` (${f})`).join('');
  const lines = VISIBLE_PROPS.filter((p) => typeof n.props[p] === 'string').map((p) => n.props[p]);
  if (n.type === 'RequiredAsterisk') return '*';
  if (BUTTONS.has(n.type) && n.type !== 'Link') return `[${squash(kids(''))}]${flags(n.props.isDisabled && 'disabled', n.props.isLoading && 'loading')}`;
  if (TEXT_INPUTS.has(n.type)) {
    const v = inputValue(n, typed);
    return `[${v === undefined || v === null || v === '' ? ' ' : String(v)}]${flags(n.props.isDisabled && 'disabled', n.props.isReadOnly && 'read-only', n.props.isInvalid && 'invalid')}`;
  }
  if (CHECKS.has(n.type)) {
    const label = typeof n.props.label === 'string' ? ` ${n.props.label}` : '';
    return `[${isChecked(n, typed) ? 'x' : ' '}]${label}${flags(n.props.isDisabled && 'disabled')}`;
  }
  if (n.type === 'Select') {
    const v = inputValue(n, typed);
    return `[${(Array.isArray(v) ? v.map(optionLabel).join(', ') : optionLabel(v))} v]${flags(n.props.isDisabled && 'disabled')}`;
  }
  if (n.type === 'RadioGroup') {
    const v = inputValue(n, typed);
    return (n.props.options ?? []).map((o) => `(${o.value === v ? '*' : ' '}) ${o.label}`).join('  ');
  }
  if (INPUTS.has(n.type)) {
    const v = inputValue(n, typed);
    return `[${v === undefined || v === null ? ' ' : (typeof v === 'object' ? JSON.stringify(v) : String(v))}]`;
  }
  if (n.type === 'DynamicTable') return [...lines, ...tableLines(n, typed)].join('\n');
  if (INLINE.has(n.type)) return [...lines, kids('')].filter((s) => s !== '').join('\n');
  if (ROW.has(n.type)) return [...lines, kids(' ')].filter((s) => s !== '').join('\n');
  return [...lines, kids('\n')].filter((s) => s !== '').join('\n');
}

// ---- outline (the tree, one node per line, for a terminal) -------------------------------------------------------
const PROP_CHARS = 60; // ratio: one prop stays readable on a terminal line; tree() always holds the whole value
function outline(n, depth = 0) {
  if (!n) return '';
  const pad = '  '.repeat(depth);
  if (n.type === 'String') return `${pad}${JSON.stringify(n.props.text)}`;
  const props = Object.entries(n.props ?? {}).map(([k, v]) => {
    if (v === FN) return `${k}=fn`;
    const s = JSON.stringify(v);
    return `${k}=${s.length > PROP_CHARS ? `${s.slice(0, PROP_CHARS)}…` : s}`;
  });
  return [`${pad}${n.type}${props.length ? ` ${props.join(' ')}` : ''}`, ...n.children.map((c) => outline(c, depth + 1))].join('\n');
}

// ---- labels --------------------------------------------------------------------------------------------------------
// A visible label names an element in three ways, ranked:
//   1. a Label whose text is the label names the element whose id is its labelFor (UI Kit's way to label Textfield,
//      TextArea, Select, ... — the reconciler prefixes both id and labelFor identically, so they still match), and a
//      string `label` prop names its own element (Toggle, Checkbox, Radio, DynamicTable);
//   2. a button's text (Button, LoadingButton, LinkButton, Pressable, Link);
//   3. a string `title`/`caption`/`aria-label` names its container (FormSection, SectionMessage, DynamicTable caption).
// Matching is exact after whitespace is collapsed. A RequiredAsterisk inside a Label is not part of its text.
const labelText = (n) => squash(n.children.filter((c) => c.type !== 'RequiredAsterisk').map((c) => textOf(c)).join(''));

function labelCandidates(doc) {
  const byId = new Map();
  const labels = [];
  const out = [];
  walk(doc, (n, ancestors) => {
    if (n.type === 'String') return;
    if (typeof n.props.id === 'string') byId.set(n.props.id, { n, ancestors });
    if (n.type === 'Label') { if (typeof n.props.labelFor === 'string') labels.push(n); return; }
    if (typeof n.props.label === 'string') out.push({ label: squash(n.props.label), node: n, ancestors, via: 'label prop', rank: 1 });
    if (BUTTONS.has(n.type)) out.push({ label: labelText(n), node: n, ancestors, via: 'button text', rank: 2 });
    for (const p of ['title', 'caption', 'aria-label']) {
      if (typeof n.props[p] === 'string') out.push({ label: squash(n.props[p]), node: n, ancestors, via: `${p} prop`, rank: 3 });
    }
  });
  for (const l of labels) {
    const t = byId.get(l.props.labelFor);
    if (t) out.push({ label: labelText(l), node: t.n, ancestors: t.ancestors, via: 'Label labelFor', rank: 1 });
  }
  return out;
}

// -> { match: {node, ancestors, via} | null, ambiguous: [..] | null, available: [labels] }
function findLabel(doc, label) {
  const want = squash(label);
  const cands = labelCandidates(doc);
  const hits = cands.filter((c) => c.label === want);
  const best = Math.min(...hits.map((h) => h.rank));
  const distinct = [...new Map(hits.filter((h) => h.rank === best).map((h) => [h.node.key, h])).values()];
  const available = [...new Set(cands.map((c) => c.label).filter(Boolean))];
  if (!distinct.length) return { match: null, ambiguous: null, available };
  if (distinct.length > 1) return { match: null, ambiguous: distinct.map((h) => `${h.node.type} (${h.via})`), available };
  return { match: distinct[0], ambiguous: null, available };
}

module.exports = { FN, snapshot, handlersOf, walk, textOf, outline, findLabel, inputValue, isChecked, labelText,
  BUTTONS, TEXT_INPUTS, CHECKS, INPUTS };
