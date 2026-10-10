// forge-2.0: the PICTURE of the UI Kit admin panel (forge2/SPEC.md §2.6). Report-only: nothing here is graded.
//
// The admin page is graded from its component tree through the kit's headless UI Kit host (lib/uikit-host), which
// renders text and never pixels: Jira's own renderer (ForgeDoc -> Atlaskit) is unpublished. The probe keeps the trees
// the admin saw and this module draws one to HTML in the look of the Atlassian design system, for the published
// pictures of what was delivered. The caption says whose drawing it is (CAPTION).
//
//   const { html, unknown, masked } = forgeDocHtml(tree, { D, typed, theme, secrets });
//   const r = await drawForgeDoc(page, kept, { D, theme, secrets, path });    // {written, reason?, unknown, masked}
//
// D is the kit's lib/uikit-host/doc.cjs: what an input shows (inputValue, isChecked), a table's display order
// (tableOf) and an undrawn component's text (textOf) come from the code the admin rows are graded with, never from a
// second copy here. `typed` is the host's value for each control the probe typed into ([[node key, value]]).
// A component type with no drawer below is drawn as a labelled box (its type name and the host's text of it) and is
// named in `unknown`: never dropped. Layout props beyond the ones read below are not drawn.
// The CI secret never reaches a picture: every text of the app passes mask-and-escape (each known secret value becomes
// a chip saying the benchmark hid it, counted in `masked`), a password field draws dots, and drawForgeDoc writes no
// file when the drawn page's text still carries a secret value (one split across elements).
// No app string reaches an attribute, a style or a URL: classes and styles are built from the tables here.

export const CAPTION = "Admin panel (UI Kit): the app's component tree, drawn by the benchmark's UI Kit host. Jira draws the same tree with its own components.";
export const WIDTH = 960;   // the picture's width in CSS px: an admin page's content column

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const HOLE = '\u0000';
const MASK = '<span class="masked">CI secret hidden by the benchmark</span>';

// Atlassian design tokens (space.*), in px.
const SPACE = { 'space.0': 0, 'space.025': 2, 'space.050': 4, 'space.075': 6, 'space.100': 8, 'space.150': 12, 'space.200': 16,
  'space.250': 20, 'space.300': 24, 'space.400': 32, 'space.500': 40, 'space.600': 48, 'space.800': 64, 'space.1000': 80 };
const ALIGN = { start: 'flex-start', center: 'center', end: 'flex-end', stretch: 'stretch', baseline: 'baseline' };
const HEADING_SIZE = { h1: 'xlarge', h2: 'large', h3: 'medium', h4: 'small', h5: 'xsmall', h6: 'xxsmall' };
const HEADING_SIZES = ['xxlarge', 'xlarge', 'large', 'medium', 'small', 'xsmall', 'xxsmall'];
const FIELD_WIDTH = { xsmall: 80, small: 160, medium: 240, large: 320, xlarge: 480 };
const TEXT_COLOR = { 'color.text': 'text', 'color.text.subtle': 'subtle', 'color.text.subtlest': 'subtlest', 'color.text.disabled': 'disabled',
  'color.text.inverse': 'inverse', 'color.text.brand': 'brand', 'color.text.selected': 'brand', 'color.link': 'link',
  'color.text.information': 'information', 'color.text.success': 'success', 'color.text.danger': 'danger',
  'color.text.warning': 'warning-text', 'color.text.discovery': 'discovery' };
const SEMANTIC = ['information', 'success', 'warning', 'danger', 'discovery', 'brand', 'neutral'];
const MESSAGE = { information: ['information', 'i'], success: ['success', null], warning: ['warning', '!'], error: ['danger', '!'], discovery: ['discovery', '?'] };
const LOZENGE = { default: 'neutral', inprogress: 'information', moved: 'warning', new: 'discovery', removed: 'danger', success: 'success' };
const BADGE = { default: 'neutral', primary: 'brand', important: 'danger', added: 'success', removed: 'danger' };
const BUTTON = ['primary', 'subtle', 'link', 'subtle-link', 'warning', 'danger'];

const CHECK = '<svg viewBox="0 0 12 12" width="10" height="10"><path d="M2 6.5l2.6 2.6L10 3.4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const CROSS = '<svg viewBox="0 0 12 12" width="8" height="8"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>';
const CHEVRON = '<svg class="chev" viewBox="0 0 16 16" width="16" height="16"><path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const SPINNER = '<svg class="spin" viewBox="0 0 16 16" width="16" height="16"><path d="M8 2a6 6 0 1 0 6 6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>';

// Solid colours only (the design system's bold tokens and its neutrals): no tinted fills, no accent rails.
const CSS = `
html[data-theme=light]{--surface:#FFFFFF;--sunken:#F1F2F4;--text:#172B4D;--subtle:#44546F;--subtlest:#626F86;--disabled:#8590A2;--border:#C1C7D0;--input-border:#8590A2;--input:#FFFFFF;--inverse:#FFFFFF;--link:#0C66E4;--brand:#0C66E4;--information:#0C66E4;--success:#1F845A;--warning:#E2B203;--warning-text:#A54800;--on-warning:#172B4D;--danger:#C9372C;--discovery:#6E5DC6;--neutral:#44546F}
html[data-theme=dark]{--surface:#1D2125;--sunken:#2C333A;--text:#C7D1DB;--subtle:#9FADBC;--subtlest:#8C9BAB;--disabled:#738496;--border:#454F59;--input-border:#738496;--input:#22272B;--inverse:#1D2125;--link:#579DFF;--brand:#579DFF;--information:#579DFF;--success:#4BCE97;--warning:#F5CD47;--warning-text:#F5CD47;--on-warning:#1D2125;--danger:#F87168;--discovery:#9F8FEF;--neutral:#9FADBC}
*{box-sizing:border-box}
body{margin:0;background:var(--surface);color:var(--text);font:14px/20px ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",Ubuntu,system-ui,"Helvetica Neue",sans-serif;-webkit-font-smoothing:antialiased}
main{width:${WIDTH}px;padding:32px 40px}
.stack{display:flex;flex-direction:column;min-width:0}
.inline{display:flex;flex-direction:row;align-items:flex-start;min-width:0}
.fill{flex:1}
.text{margin:0;overflow-wrap:anywhere}
.heading{margin:0;font-weight:700;color:var(--text);overflow-wrap:anywhere}
.hz-xxlarge{font-size:32px;line-height:36px}.hz-xlarge{font-size:28px;line-height:32px}.hz-large{font-size:24px;line-height:28px}
.hz-medium{font-size:20px;line-height:24px}.hz-small{font-size:16px;line-height:20px}.hz-xsmall{font-size:14px;line-height:20px}.hz-xxsmall{font-size:12px;line-height:16px}
.label{display:block;font-size:12px;line-height:16px;font-weight:600;color:var(--subtle);margin-bottom:4px}
.req{color:var(--danger);padding-left:2px}
.helper{font-size:12px;line-height:16px;color:var(--subtlest);margin-top:4px}.helper.error{color:var(--danger)}.helper.valid{color:var(--success)}
.link{color:var(--link);text-decoration:underline}
.code{font:12px/16px ui-monospace,SFMono-Regular,Menlo,monospace;border:1px solid var(--border);border-radius:3px;padding:1px 4px;overflow-wrap:anywhere}
pre.code{margin:0;padding:8px 12px;white-space:pre-wrap}
.field{display:flex;align-items:center;gap:6px;width:100%;min-height:40px;padding:6px 8px;border:1px solid var(--input-border);border-radius:3px;background:var(--input);color:var(--text)}
.field.compact{min-height:32px;padding:2px 8px}.field.area{align-items:flex-start;min-height:84px}
.field.invalid{border:2px solid var(--danger)}
.field.disabled{background:var(--sunken);border-color:var(--sunken);color:var(--disabled)}
.field .value{flex:1;min-width:0;white-space:pre-wrap;overflow-wrap:anywhere}
.field .placeholder{color:var(--subtlest)}
.field .kind{font-size:11px;line-height:16px;color:var(--subtlest);white-space:nowrap}
.chev{flex:none;color:var(--subtle)}
.chip{display:inline-flex;align-items:center;height:20px;padding:0 6px;border:1px solid var(--input-border);border-radius:3px;white-space:nowrap}
.toggle{display:inline-flex;align-items:center;justify-content:space-between;flex:none;width:32px;height:16px;padding:2px;border-radius:8px;background:var(--neutral);color:var(--inverse)}
.toggle.on{background:var(--success);flex-direction:row-reverse}
.toggle i{display:block;width:12px;height:12px;border-radius:50%;background:var(--inverse)}
.toggle b{display:flex;align-items:center;justify-content:center;width:12px;height:12px}
.toggle.large{width:40px;height:20px;border-radius:10px}.toggle.large i,.toggle.large b{width:16px;height:16px}
.toggle.disabled{background:var(--disabled)}
.choice{display:flex;align-items:center;gap:8px}.choices{display:flex;flex-direction:column;gap:4px}
.box{display:inline-flex;align-items:center;justify-content:center;flex:none;width:16px;height:16px;border:2px solid var(--input-border);border-radius:3px;background:var(--input);color:var(--inverse)}
.box.round{border-radius:50%}.box.on{background:var(--brand);border-color:var(--brand)}
.box.round.on{background:var(--inverse);border:5px solid var(--brand)}
.choice.disabled{color:var(--disabled)}.choice.disabled .box{border-color:var(--disabled)}.choice.disabled .box.on{background:var(--disabled)}
.btn{display:inline-flex;align-items:center;justify-content:center;gap:6px;min-height:32px;padding:4px 12px;border:1px solid var(--input-border);border-radius:3px;background:var(--surface);color:var(--text);font-weight:500;text-align:center}
.btn.primary{background:var(--brand);border-color:var(--brand);color:var(--inverse)}
.btn.danger{background:var(--danger);border-color:var(--danger);color:var(--inverse)}
.btn.warning{background:var(--warning);border-color:var(--warning);color:var(--on-warning)}
.btn.subtle{border-color:transparent;background:transparent}
.btn.link,.btn.subtle-link{border-color:transparent;background:transparent;color:var(--link);padding:4px 0}
.btn.selected{background:var(--neutral);border-color:var(--neutral);color:var(--inverse)}
.btn.disabled{background:var(--sunken);border-color:var(--sunken);color:var(--disabled)}
.btn.compact{min-height:24px;padding:0 12px}.btn.fit{display:flex;width:100%}
.glyph{font:11px/16px ui-monospace,SFMono-Regular,Menlo,monospace;border:1px solid currentColor;border-radius:3px;padding:0 4px}
.spin{flex:none}
.c-information{--c:var(--information);--on:var(--inverse)}.c-success{--c:var(--success);--on:var(--inverse)}.c-warning{--c:var(--warning);--on:var(--on-warning)}
.c-danger{--c:var(--danger);--on:var(--inverse)}.c-discovery{--c:var(--discovery);--on:var(--inverse)}.c-brand{--c:var(--brand);--on:var(--inverse)}.c-neutral{--c:var(--neutral);--on:var(--inverse)}
.message{display:flex;gap:16px;padding:16px;border:2px solid var(--c);border-radius:3px;background:var(--surface)}
.message-icon{display:flex;align-items:center;justify-content:center;flex:none;width:24px;height:24px;border-radius:50%;background:var(--c);color:var(--on);font-weight:700}
.message-icon svg{width:14px;height:14px}
.message-body{display:flex;flex-direction:column;gap:8px;flex:1;min-width:0}.message-title{font-size:16px;line-height:24px;font-weight:600}
.actions{display:flex;flex-wrap:wrap;gap:16px}
.lozenge{display:inline-flex;align-items:center;min-height:20px;padding:0 6px;border:1px solid var(--c);border-radius:3px;background:var(--surface);color:var(--c);font-size:12px;line-height:16px;font-weight:700}
.lozenge.c-warning{color:var(--warning-text)}.lozenge.bold{background:var(--c);color:var(--on)}
.badge{display:inline-flex;justify-content:center;min-width:24px;padding:0 6px;border-radius:8px;background:var(--c);color:var(--on);font-size:12px;line-height:16px}
.fill-c{background:var(--c);color:var(--on)}.edge-c{border:2px solid var(--c);border-radius:3px}.sunken{background:var(--sunken)}
table.dt{width:100%;border-collapse:collapse}
.dt caption{caption-side:top;text-align:left;font-size:20px;line-height:24px;font-weight:600;padding-bottom:8px}
.dt th{text-align:left;font-size:12px;line-height:16px;font-weight:700;color:var(--subtle);padding:4px 8px;border-bottom:2px solid var(--border);overflow-wrap:anywhere}
.dt td{padding:8px;vertical-align:top;overflow-wrap:anywhere}
.dt th:first-child,.dt td:first-child{padding-left:0}
.dt tbody{border-bottom:2px solid var(--border)}
.dt .empty td{text-align:center;padding:24px 0}
.note{margin-top:8px;font-size:12px;line-height:16px;color:var(--subtlest)}
.progress{height:6px;border-radius:3px;background:var(--border)}.progress i{display:block;height:6px;border-radius:3px;background:var(--neutral)}.progress.success i{background:var(--success)}
.form{display:flex;flex-direction:column}.form-header,.form-section{display:flex;flex-direction:column;gap:8px}.form-section,.form-footer{margin-top:24px}
.form-footer{display:flex;gap:8px;justify-content:flex-end}.form-footer.start{justify-content:flex-start}
.title-l{font-size:20px;line-height:24px;font-weight:600}.title-m{font-size:16px;line-height:20px;font-weight:600}.subtle{color:var(--subtle)}
.empty-state{display:flex;flex-direction:column;align-items:center;gap:16px;padding:32px 0;text-align:center}
.list{margin:0;padding-left:24px}
.unknown{display:flex;flex-direction:column;align-items:flex-start;gap:6px;padding:8px;border:2px dashed var(--danger);border-radius:3px;white-space:pre-wrap;overflow-wrap:anywhere}
.unknown-type{background:var(--danger);color:var(--inverse);font:700 11px/16px ui-monospace,SFMono-Regular,Menlo,monospace;padding:1px 6px;border-radius:3px}
.masked{display:inline-block;background:var(--text);color:var(--surface);font:700 11px/16px ui-monospace,SFMono-Regular,Menlo,monospace;padding:1px 6px;border-radius:3px}
`;

export function forgeDocHtml(tree, { D, typed = [], theme = 'light', secrets = [] }) {
  const typedMap = new Map(typed);
  const unknown = new Set();
  let masked = 0;
  // longest first: a value that contains another is hidden whole
  const hidden = [...new Set(secrets)].filter((s) => typeof s === 'string' && s !== '').sort((a, b) => b.length - a.length);
  const t = (s) => {
    let out = String(s ?? '').replaceAll(HOLE, '');
    for (const v of hidden) {
      const parts = out.split(v);
      masked += parts.length - 1;
      out = parts.join(HOLE);
    }
    return esc(out).replaceAll(HOLE, MASK);
  };
  const el = (tag, cls, inner, style = '') => `<${tag} class="${cls}"${style ? ` style="${style}"` : ''}>${inner}</${tag}>`;
  const on = (flag, cls) => (flag ? ` ${cls}` : '');
  const str = (n, p, cls) => (typeof n.props[p] === 'string' ? el('div', cls, t(n.props[p])) : '');
  const draw = (n) => (Object.hasOwn(DRAW, n.type) ? DRAW[n.type] : D.INPUTS.has(n.type) ? otherInput : undrawn)(n);
  const kids = (n) => n.children.map(draw).join('');
  // A React element given as a prop crosses the bridge as {type, props} with its children inside props (doc.cjs cloneValue).
  const asNodes = (v) => {
    if (v === null || v === undefined || typeof v === 'boolean') return [];
    if (Array.isArray(v)) return v.flatMap(asNodes);
    if (typeof v !== 'object') return [{ type: 'String', props: { text: String(v) }, children: [] }];
    const { children, ...props } = v.props ?? {};
    return [{ type: String(v.type), props, children: asNodes(children) }];
  };
  const elems = (v) => asNodes(v).map(draw).join('');

  const flex = (n, cls, main, cross) => {
    const style = [`gap:${SPACE[n.props.space] ?? 0}px`,
      n.props.spread === 'space-between' ? 'justify-content:space-between' : ALIGN[n.props[main]] && `justify-content:${ALIGN[n.props[main]]}`,
      ALIGN[n.props[cross]] && `align-items:${ALIGN[n.props[cross]]}`, n.props.shouldWrap && 'flex-wrap:wrap',
      n.props.rowSpace in SPACE && `row-gap:${SPACE[n.props.rowSpace]}px`].filter(Boolean).join(';');
    return el('div', cls + on(n.props.grow === 'fill', 'fill'), kids(n), style);
  };
  const box = (n) => {
    const props = { ...(n.props.xcss && typeof n.props.xcss === 'object' && !Array.isArray(n.props.xcss) ? n.props.xcss : {}), ...n.props };
    const style = [['padding', 'padding'], ['paddingBlock', 'padding-block'], ['paddingInline', 'padding-inline'], ['paddingBlockStart', 'padding-block-start'],
      ['paddingBlockEnd', 'padding-block-end'], ['paddingInlineStart', 'padding-inline-start'], ['paddingInlineEnd', 'padding-inline-end']]
      .filter(([p]) => props[p] in SPACE).map(([p, css]) => `${css}:${SPACE[props[p]]}px`).join(';');
    const bg = String(props.backgroundColor ?? '');
    const semantic = SEMANTIC.find((s) => bg.startsWith(`color.background.${s === 'neutral' ? 'neutral.bold' : s}`));
    const cls = !semantic ? on(/^(color\.background\.neutral|elevation\.surface\.sunken)/.test(bg), 'sunken')
      : ` c-${semantic} ${bg.endsWith('.bold') ? 'fill-c' : 'edge-c'}`;
    return el('div', `boxed${cls}`, kids(n), style);
  };
  const text = (n) => {
    const tag = { em: 'em', strong: 'strong', span: 'span' }[n.props.as] ?? 'p';
    const style = [{ small: 'font-size:12px;line-height:16px', large: 'font-size:16px;line-height:24px' }[n.props.size],
      { medium: 'font-weight:500', semibold: 'font-weight:600', bold: 'font-weight:700' }[n.props.weight],
      TEXT_COLOR[n.props.color] && `color:var(--${TEXT_COLOR[n.props.color]})`,
      ['center', 'end'].includes(n.props.align) && `text-align:${n.props.align}`].filter(Boolean).join(';');
    return el(tag, 'text', kids(n), style);
  };
  const heading = (n) => {
    const as = /^h[1-6]$/.test(n.props.as) ? n.props.as : 'div';
    return el(as, `heading hz-${HEADING_SIZES.includes(n.props.size) ? n.props.size : HEADING_SIZE[as] ?? 'medium'}`, kids(n));
  };
  const button = (n) => {
    const glyph = (p) => (typeof n.props[p] === 'string' ? el('span', 'glyph', t(n.props[p])) : '');
    return el('span', `btn ${BUTTON.includes(n.props.appearance) ? n.props.appearance : 'default'}${on(n.props.isSelected, 'selected')}${on(n.props.isDisabled, 'disabled')}`
      + `${on(n.props.spacing === 'compact', 'compact')}${on(n.props.shouldFitContainer, 'fit')}`,
    (n.props.isLoading ? SPINNER : '') + glyph('iconBefore') + kids(n) + glyph('iconAfter'));
  };
  const field = (n, inner, cls = '') => {
    const w = n.props.width;
    const width = Number.isFinite(w) ? Math.min(Math.max(w, 0), WIDTH) : FIELD_WIDTH[w];
    return el('div', `field${cls}${on(n.props.isCompact, 'compact')}${on(n.props.isInvalid, 'invalid')}${on(n.props.isDisabled, 'disabled')}`, inner, width ? `width:${width}px` : '');
  };
  // What the person reads in an input: its value, else its placeholder; a password field shows dots, as a browser does.
  const shown = (n, v) => {
    const s = v === undefined || v === null ? '' : typeof v === 'object' ? JSON.stringify(v) : String(v);
    if (s === '') return el('span', `value${on(typeof n.props.placeholder === 'string', 'placeholder')}`, t(n.props.placeholder ?? ''));
    return el('span', 'value', n.props.type === 'password' ? '•'.repeat([...s].length) : t(s));
  };
  const optionLabel = (o) => (o && typeof o === 'object' ? String(o.label ?? o.value ?? '') : String(o ?? ''));
  const select = (n) => {
    const v = D.inputValue(n, typedMap);
    const inner = Array.isArray(v) ? el('span', 'value', v.map((o) => el('span', 'chip', t(optionLabel(o)))).join(' ')) : shown(n, v === undefined || v === null ? v : optionLabel(v));
    return field(n, inner + CHEVRON);
  };
  const choice = (label, isOn, round, disabled) => el('span', `choice${on(disabled, 'disabled')}`,
    el('span', `box${on(round, 'round')}${on(isOn, 'on')}`, isOn && !round ? CHECK : '') + (label === undefined ? '' : el('span', '', t(label))));
  const toggle = (n) => {
    const isOn = D.isChecked(n, typedMap);
    return el('span', `toggle${on(isOn, 'on')}${on(n.props.size === 'large', 'large')}${on(n.props.isDisabled, 'disabled')}`, `<i></i><b>${isOn ? CHECK : CROSS}</b>`);
  };
  // The display order is the kit's own (doc.cjs tableOf sorts as ADS does, by the sort column's cell keys): each row
  // gets one more cell holding its index, past every column the sort can read, and the order tableOf returns the rows
  // in is the order drawn.
  const ordered = (table, rows) => {
    const stamp = (i) => ({ type: 'Cell', props: {}, children: [{ type: 'String', props: { text: String(i) }, children: [] }] });
    const stamped = { ...table, children: table.children.map((part) => (part.type === 'ContentWrapper' && part.props.name === 'rows'
      ? { ...part, children: rows.map((r, i) => ({ ...r, children: [...r.children.map((c) => ({ ...c, children: [] })), stamp(i)] })) } : part)) };
    return D.tableOf(stamped).rows.map((cells) => rows[Number(cells[cells.length - 1])]);
  };
  const table = (n) => {
    const part = (name) => n.children.find((c) => c.type === 'ContentWrapper' && c.props.name === name)?.children ?? [];
    const head = part('head');
    const rows = ordered(n, part('rows'));
    const span = (c) => (Number.isInteger(c.props.colSpan) ? ` colspan="${c.props.colSpan}"` : '');
    const sortKey = n.props.sortKey ?? n.props.defaultSortKey;
    const arrow = (n.props.sortOrder ?? n.props.defaultSortOrder) === 'DESC' ? ' ↓' : ' ↑';
    const th = head.map((c) => `<th${span(c)}>${kids(c)}${sortKey !== undefined && c.props.cellKey === sortKey ? arrow : ''}</th>`).join('');
    const body = rows.length ? rows.map((r) => `<tr>${r.children.map((c) => `<td${span(c)}>${kids(c)}</td>`).join('')}</tr>`).join('')
      : n.props.emptyView === undefined ? '' : `<tr class="empty"><td colspan="${Math.max(1, head.length)}">${elems(n.props.emptyView)}</td></tr>`;
    const paged = Number.isInteger(n.props.rowsPerPage) && rows.length > n.props.rowsPerPage
      ? el('div', 'note', `Jira pages this table at ${n.props.rowsPerPage} rows; every row is drawn here.`) : '';
    return `<div><table class="dt">${typeof n.props.caption === 'string' ? `<caption>${t(n.props.caption)}</caption>` : ''}`
      + `${th ? `<thead><tr>${th}</tr></thead>` : ''}<tbody>${body}</tbody></table>${n.props.isLoading ? SPINNER : ''}${paged}</div>`;
  };
  const message = (n) => {
    const [colour, icon] = MESSAGE[n.props.appearance] ?? MESSAGE.information;
    return el('div', `message c-${colour}`, el('span', 'message-icon', icon ?? CHECK) + el('div', 'message-body', str(n, 'title', 'message-title') + kids(n)
      + (n.props.actions === undefined ? '' : el('div', 'actions', elems(n.props.actions)))));
  };
  const toned = (n, cls, tones) => el('span', `${cls} c-${tones[n.props.appearance] ?? 'neutral'}${on(n.props.isBold, 'bold')}`, kids(n));
  const helper = (cls) => (n) => el('div', `helper${cls}`, kids(n));
  const inline = (tag, cls = '') => (n) => el(tag, cls, kids(n));

  const DRAW = {
    String: (n) => t(n.props.text),
    Root: inline('div'),
    Stack: (n) => flex(n, 'stack', 'alignBlock', 'alignInline'),
    Inline: (n) => flex(n, 'inline', 'alignInline', 'alignBlock'),
    ButtonGroup: (n) => el('div', 'inline', kids(n), 'gap:4px;flex-wrap:wrap'),
    TagGroup: (n) => el('div', 'inline', kids(n), 'gap:4px;flex-wrap:wrap'),
    Box: box,
    Text: text,
    Heading: heading,
    Strong: inline('strong'), Em: inline('em'), Strike: inline('s'),
    Code: inline('code', 'code'),
    CodeBlock: (n) => el('pre', 'code', t(n.props.text ?? '') + kids(n)),
    Link: inline('span', 'link'),
    Label: inline('label', 'label'),
    RequiredAsterisk: () => el('span', 'req', '*'),
    HelperMessage: helper(''), ErrorMessage: helper(' error'), ValidMessage: helper(' valid'),
    Tooltip: kids,   // its content shows on hover only
    Lozenge: (n) => toned(n, 'lozenge', LOZENGE),
    Badge: (n) => el('span', `badge c-${BADGE[n.props.appearance] ?? 'neutral'}`, kids(n)),
    Tag: (n) => el('span', 'chip', t(n.props.text ?? '') + kids(n)),
    Button: button, LoadingButton: button, LinkButton: button, Pressable: button,
    SectionMessage: message,
    SectionMessageAction: inline('span', 'link'),
    Spinner: () => SPINNER,
    ProgressBar: (n) => el('div', `progress${on(n.props.appearance === 'success', 'success')}`,
      `<i style="width:${Math.round(Math.min(1, Math.max(0, Number(n.props.value) || 0)) * 100)}%"></i>`),
    EmptyState: (n) => {
      const actions = elems(n.props.primaryAction) + elems(n.props.secondaryAction);
      return el('div', 'empty-state', str(n, 'header', 'title-l') + str(n, 'description', 'subtle') + kids(n) + (actions ? el('div', 'actions', actions) : ''));
    },
    Form: inline('div', 'form'),
    FormHeader: (n) => el('div', 'form-header', str(n, 'title', 'title-l') + str(n, 'description', 'subtle') + kids(n)),
    FormSection: (n) => el('div', 'form-section', str(n, 'title', 'title-m') + str(n, 'description', 'subtle') + kids(n)),
    FormFooter: (n) => el('div', `form-footer${on(n.props.align === 'start', 'start')}`, kids(n)),
    Textfield: (n) => field(n, elems(n.props.elemBeforeInput) + shown(n, D.inputValue(n, typedMap)) + elems(n.props.elemAfterInput)),
    TextArea: (n) => field(n, shown(n, D.inputValue(n, typedMap)), ' area'),
    Select: select,
    Toggle: toggle,
    Checkbox: (n) => choice(typeof n.props.label === 'string' ? n.props.label : undefined, D.isChecked(n, typedMap), false, n.props.isDisabled),
    RadioGroup: (n) => el('div', 'choices', (Array.isArray(n.props.options) ? n.props.options : [])
      .map((o) => choice(o?.label ?? o?.value ?? '', o?.value === D.inputValue(n, typedMap), true, n.props.isDisabled || o?.isDisabled)).join('')),
    DynamicTable: table,
    List: (n) => el(n.props.type === 'ordered' ? 'ol' : 'ul', 'list', kids(n)),
    ListItem: inline('li'),
  };
  // the kit's other inputs (DatePicker, TimePicker, Range, UserPicker): the value the host reads, and which control it is
  const otherInput = (n) => field(n, shown(n, D.inputValue(n, typedMap)) + el('span', 'kind', t(n.type)));
  const undrawn = (n) => {
    unknown.add(n.type);
    return el('div', 'unknown', el('span', 'unknown-type', `${t(n.type)}: not drawn by the benchmark's renderer`) + t(D.textOf(n, typedMap)));
  };

  const html = `<!doctype html><html data-theme="${theme === 'dark' ? 'dark' : 'light'}"><head><meta charset="utf-8"><style>${CSS}</style></head>`
    + `<body><main>${draw(tree)}</main></body></html>`;
  return { html, unknown: [...unknown].sort(), masked };
}

// Draws one kept tree ({tree, typed}) on `page` and writes `path`. No file is written when the drawn text still
// carries a secret value: whitespace is ignored on both sides, so a value split across elements is found.
export async function drawForgeDoc(page, kept, { D, theme, secrets, path }) {
  const { html, unknown, masked } = forgeDocHtml(kept.tree, { D, typed: kept.typed, theme, secrets });
  await page.setViewportSize({ width: WIDTH, height: 600 });
  await page.setContent(html, { waitUntil: 'load' });
  const flat = (s) => String(s).replace(/\s+/g, '');
  const drawn = flat(await page.evaluate(() => document.querySelector('main').textContent));
  if (secrets.some((s) => typeof s === 'string' && flat(s) !== '' && drawn.includes(flat(s)))) {
    return { written: false, reason: 'the drawn panel still carried a CI secret value after masking', unknown, masked };
  }
  await page.screenshot({ path, fullPage: true });
  return { written: true, unknown, masked };
}
