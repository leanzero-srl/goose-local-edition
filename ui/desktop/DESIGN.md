# LeanZero Studio — design system v1

The visual contract for the Goose Flock desktop remake. Tokens live in `src/styles/main.css`
(the `LEANZERO STUDIO` block), the class-name helpers in `src/components/lz/tokens.ts`, the
primitives in `src/components/lz/`. Every value here is measured against the real Tailwind
pipeline by `src/components/lz/*.test.ts(x)`; a class that compiles to nothing, a faded tint, a
left rail or a native control fails the build.

Three commitments: **solid colour used with meaning** (one accent, a status triad, a node ramp —
nothing else is coloured), **type carries the hierarchy** (size, weight and case; never a coloured
square or a rail), **one layer of chrome** (1px hairlines, radius 6/10, no decorative shadow).

## Palette

Slate neutrals shared with leanzero.net. Every value is a solid 6-digit hex in both themes — no
alpha, no `color-mix`, no pastel. Tokens resolve under `.local-edition` (set on `<html>` by
`EditionContext`); outside it each surface falls back to the nearest host-theme token so a Studio
primitive never renders transparent.

| token                      | utility                   | light     | dark      | use                                                                                                         |
| -------------------------- | ------------------------- | --------- | --------- | ----------------------------------------------------------------------------------------------------------- |
| `--color-lz-bg`            | `bg-lz-bg`                | `#f8fafc` | `#0b1220` | the page                                                                                                    |
| `--color-lz-surface`       | `bg-lz-surface`           | `#ffffff` | `#0f172a` | cards, inputs, tables                                                                                       |
| `--color-lz-surface-2`     | `bg-lz-surface-2`         | `#f1f5f9` | `#1e293b` | hover, inset wells, disabled fill                                                                           |
| `--color-lz-border`        | `border-lz-border`        | `#e2e8f0` | `#1e293b` | hairlines, card edges                                                                                       |
| `--color-lz-border-strong` | `border-lz-border-strong` | `#cbd5e1` | `#334155` | control outlines, quiet chips                                                                               |
| `--color-lz-ink`           | `text-lz-ink`             | `#0f172a` | `#f8fafc` | titles, values, body                                                                                        |
| `--color-lz-ink-2`         | `text-lz-ink-2`           | `#334155` | `#cbd5e1` | secondary body, zone headers                                                                                |
| `--color-lz-ink-3`         | `text-lz-ink-3`           | `#64748b` | `#94a3b8` | META only: labels, counts, timestamps, quiet chips (4.8:1 on the surface, 4.3:1 on hover — never body copy) |
| `--color-lz-ink-4`         | `text-lz-ink-4`           | `#94a3b8` | `#64748b` | placeholders and the "—" of an absent value (2.6:1 — never information)                                     |

**Accent** — one per view, always with white ink.

| token                                                       | utility                                          | light     | dark      |
| ----------------------------------------------------------- | ------------------------------------------------ | --------- | --------- |
| `--color-lz-accent` (= the existing `--color-action-solid`) | `bg-lz-accent` `text-lz-accent` `ring-lz-accent` | `#1d4ed8` | `#1d4ed8` |
| `--color-lz-accent-hover`                                   | `hover:bg-lz-accent-hover`                       | `#1e40af` | `#2563eb` |
| `--color-lz-accent-ink`                                     | `text-lz-accent-ink`                             | `#ffffff` | `#ffffff` |

**Secondary accent** — SPARING. Reserved for the reasoning/thinking channel and at most one
secondary emphasis per view. Never two accents in one component.

| token                      | utility                               | light     | dark      |
| -------------------------- | ------------------------------------- | --------- | --------- |
| `--color-lz-secondary`     | `bg-lz-secondary` `text-lz-secondary` | `#6d28d9` | `#c084fc` |
| `--color-lz-secondary-ink` | `text-lz-secondary-ink`               | `#ffffff` | `#0b1220` |

(The brief's dark `#a855f7` measured 4.27:1 as text on the dark surface; `#c084fc` measures 6.4:1
both as text and as a fill under dark ink.)

**Status triad** — the existing tokens, aliased. The bare step is for text, icons and dots; the
`-solid` step is a FILL that carries white text (one shade darker so the white clears AA).

| tone    | text / dot                        | fill                             | light                 | dark (text) |
| ------- | --------------------------------- | -------------------------------- | --------------------- | ----------- |
| ok      | `text-lz-ok` `bg-lz-ok`           | `bg-lz-ok-solid text-white`      | `#16a34a` / `#15803d` | `#22c55e`   |
| warn    | `text-lz-warn` `bg-lz-warn`       | `bg-lz-warn-solid text-white`    | `#d97706` / `#b45309` | `#f59e0b`   |
| err     | `text-lz-err` `bg-lz-err`         | `bg-lz-err-solid text-white`     | `#dc2626` / `#dc2626` | `#ef4444`   |
| stopped | `text-lz-stopped` `bg-lz-stopped` | `bg-lz-stopped-solid text-white` | `#475569` / `#475569` | `#94a3b8`   |

**Node ramp** — `bg-lz-node-1…6` with `text-lz-node-N-ink` (the ink was measured per hue: white on
blue/violet/pink, near-black on cyan/orange/green in light; near-black on all six in dark). Node
hue is NODE IDENTITY ONLY — never chrome, never a zone, never a status.

## Typography

Inter, bundled offline via `@fontsource/inter` (400/500/600/700), app-wide through the Tailwind
preflight (`--default-font-family`). Features `cv11` (single-storey a) and `ss01` (open digits)
are on by default; `tnum` (tabular figures) is the `tnum` utility and is on in every numeric
column, count pill and KeyValue value. Mono is `ui-monospace, "SF Mono", Menlo, monospace`
(`font-mono`).

| step    | utility                  | size / weight / tracking / leading | ink                            | use                                                           |
| ------- | ------------------------ | ---------------------------------- | ------------------------------ | ------------------------------------------------------------- |
| display | `text-lz-display`        | 28 / 600 / −0.02em / 1.2           | ink                            | page title, EmptyState title                                  |
| h1      | `text-lz-h1`             | 22 / 600 / −0.01em / 1.25          | ink                            | a dialog or a sub-view title                                  |
| h2      | `text-lz-h2`             | 16 / 600 / 0 / 1.35                | ink                            | card titles that are not zones                                |
| body    | `text-lz-body`           | 13 / 400 / 0 / 1.5                 | ink (`bodyMuted`: ink-2)       | everything a person reads                                     |
| meta    | `text-lz-meta`           | 11 / 500 / 0 / 1.4                 | ink-3                          | labels, counts, timestamps, chips                             |
| zone    | `text-lz-zone uppercase` | 11 / 600 / +0.08em / 1.2           | ink-2 (ink-3 in table headers) | section headers, column headers — the ONLY uppercase register |
| mono    | `font-mono text-lz-mono` | 12 / 400 / 0 / 1.5                 | ink                            | paths, hashes, ids, log lines                                 |

Each step is ONE utility carrying size, weight, tracking and leading (`TYPE.display` etc. in
tokens.ts adds the ink). Emphasis inside a step: `WEIGHT.medium` / `WEIGHT.semibold`
(`font-lz-medium` / `font-lz-semibold`).

**Measured:** `font-medium`, `font-semibold`, `font-bold` and `font-normal` compile to NO rule in
this app — the MCP theme registration sets `--font-weight-*` to `initial`. Every such class in
the existing app is a silent no-op; the Studio never uses them. (Re-measured 2026-09-02 through
`candidatesToCss`: the no-op set is exactly those four; `font-extrabold` DOES emit a rule. The rule
stands regardless — never rely on any host `font-*` weight utility; use `font-lz-medium` /
`font-lz-semibold`, which resolve to the Studio tokens in both editions.)

## Rhythm, radius, motion, elevation

- 4px base, 8px rhythm. `p-lz-page` 32 · `gap-lz-section` 24 · `p-lz-card` 16 · rows `h-lz-row` 36 /
  `h-lz-row-dense` 32 · controls 32 (`h-8`) / small 28 (`h-7`).
- Radius: `rounded-lz-control` 6 (buttons, inputs, chips, segmented) · `rounded-lz-card` 10 (panels,
  the EmptyState icon block) · `rounded-lz-pill` 999 (count pills, dots).
- Borders are 1px, always (`border`, `border-t`, `border-b`) — never a left border.
- Elevation: ONE token, for overlays only (popover, menu, dialog): `shadow-lz-overlay
dark:shadow-lz-overlay-dark` (`0 8px 24px rgba(15,23,42,.12)` / `rgba(0,0,0,.55)`). Cards have
  no shadow. (Tailwind inlines shadow values, which is why the dark twin is a second token.)
- Motion: 120ms ease-out on colour only — `MOTION` = `transition-colors duration-120 ease-lz`.
  The live pulse (`animate-lz-live`) SCALES a solid dot 1 → 1.4; it never fades. The app's
  `prefers-reduced-motion` rule disables it.

## States

- Hover: a SOLID step to surface-2 (`SURFACE.hover`). Never an opacity, never a tint.
- Selected: the accent fill with white ink (`SURFACE.selected`) — rows, segments — or a 2px accent
  INSET ring (`SURFACE.selectedRing`) where the fill would hide content. Hover on a selected row is
  the accent-hover step (`SURFACE.selectedHover`), never the neutral step: selected always wins.
- Focus: the app's own accent ring on `:focus-visible` (`FOCUS` = 2px outline, `--color-ring`).
- Disabled: solid — surface-2 fill, ink-3 text, hairline border, pointer-events off (`DISABLED`).
  Never `opacity-50`.

## Primitives (`src/components/lz`)

Import from `./components/lz` (the barrel). All are function components, typed, theme-aware,
`className` pass-through, no state of their own except what a control needs to be accessible.

| primitive       | API                                                                                                                                                              | rule                                                                                                                                                                                                                       |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PageHeader`    | `title`, `eyebrow?`, `subtitle?`, `actions?`                                                                                                                     | one per view; the `title` is the display step; `actions` holds the view's primary Button and/or a Segmented                                                                                                                |
| `SectionHeader` | `title`, `count?`, `right?`, `as? = 'h2'`                                                                                                                        | the zone register; `count` MUST be the length of the list the body renders (a header counts what the body shows); no coloured square                                                                                       |
| `Segmented<V>`  | `options[{value,label,icon?,disabled?}]`, `value`, `onChange`, `aria-label`, `size?`                                                                             | controlled single choice; a radiogroup with roving focus (arrows move+select, Home/End); active = accent fill                                                                                                              |
| `Button`        | `variant? = 'secondary' \| 'primary' \| 'ghost'`, `size? = 'md' \| 'sm'`, `icon?`, native button props                                                           | ONE `primary` per view; `type` defaults to `button`; disabled is solid                                                                                                                                                     |
| `Chip`          | `tone?`, `node?`, `icon?`, `title?`, children                                                                                                                    | QUIET by default (outline, ink-3, no fill) for metadata; `tone` for STATE; `node` for identity; never uppercase; never pile chips where a table column will do                                                             |
| `StatusDot`     | `tone? = 'stopped'`, `node?`, `live?`, `label` (required), `size? = 8`                                                                                           | the colour is the mark, the `label` is the meaning; `live` pulses by scale                                                                                                                                                 |
| `DataTable<T>`  | `columns[{key,header,cell,align?,width?,numeric?}]`, `rows`, `rowKey` (required), `dense?`, `selectedKey?`, `onRowClick?`, `rowAction?`, `empty?`, `aria-label?` | rows keyed by identity, never index; `numeric` = right-aligned tabular figures; always pass `empty` (an EmptyState) — a header over nothing reads as a bug; put the row COUNT in the SectionHeader above, not in the table |
| `EmptyState`    | `icon?`, `title`, `body?`, `action?`                                                                                                                             | centered, max-width 440, the icon in a solid accent block; the `action` is the one primary Button                                                                                                                          |
| `KeyValue`      | `items[{key,label,value,tone?,mono?}]`, `dense?`, `aria-label?`                                                                                                  | status panels; values right-aligned tnum; `tone` colours a value by meaning; `mono` for ids/paths                                                                                                                          |
| `Toolbar`       | `search?{value,onChange,placeholder?,aria-label}`, `filters?`, `actions?`, `aria-label?`                                                                         | one 36px row; the search is a plain text input with its own clear button (never `type=search`, never a native select in `filters`)                                                                                         |
| `Panel`         | `title?`, `count?`, `headerRight?`, `header?`, `padded? = true`, children                                                                                        | the surface card; `padded={false}` under a DataTable; a custom `header` wins over `title`                                                                                                                                  |

**Buttons.** A button's own label never truncates or ellipsizes; wrap the row or drop to icon+title at the breakpoint.

`tokens.ts` helpers: `cx(...)` (plain join), `toneClasses(tone, 'fill'|'text'|'dot')`,
`nodeClasses(node, register)`, and the constant maps `TONE_*`, `NODE_*`, `TYPE`, `SURFACE`,
`RADIUS`, `ROW`, `SPACE`, `WEIGHT`, `FOCUS`, `MOTION`, `DISABLED`, `TNUM`.

**Never run Studio classes through `cn()` / tailwind-merge.** Measured:
`twMerge('text-lz-display text-lz-ink')` returns `text-lz-ink` — it classifies both as a text
colour and deletes the size step. Use `cx`.

## Bans (absolute)

1. No left accent rail: no `border-l`, `border-l-*`, `border-left`, no coloured strip pinned to the
   left edge of anything.
2. No faded colour: no `/10 /15 /20` (or any) opacity modifier on an accent, no `opacity-*` on
   content, no `color-mix`, no alpha fills, no pastel. Solid, saturated colour, used with meaning.
3. No native browser primitives: no `<select>`, no `window.alert/confirm/prompt`, no
   `type="search"` chrome. Custom controls on the tokens, always.
4. No decorative shadow; no gradient; no second accent in a component; no node hue on chrome.
5. No hand-written colour: every hue is a token utility through `tokens.ts`.

`src/components/lz/assertStudioClean.ts` refuses 1–3 on rendered output; `studioGallery.test.tsx`
compiles every emitted class against the real pipeline so a dead utility cannot ship.

## How the website can follow it

leanzero.net already sets Inter on the same slate ramp with `#1d4ed8` as its blue. Following the
Studio means: the nine neutrals above as the site's surface/ink scale, the type steps as the site's
headings/body/meta (28/22/16/13/11 with the same weights and tracking), `#6d28d9` only for one
secondary emphasis per page, the status triad for any state colour, 1px hairlines and radius 6/10,
hover as a solid step to `#f1f5f9`, and the same bans. The tables in this file are the source; the
CSS variables in `main.css` are copy-ready.

## The brand mark

ONE geometry, `src/components/icons/leanzeroMark.tsx`, drawn in `currentColor` — a big "L" (the
leanzero.net monogram language: sharp corners, one weight) with TWO of the ORIGINAL goose flying out
of it. Four owner rounds got here; the last two are the load-bearing ones: *"take the original shape
of the goose from the original icon ... so people know that this was part of the original goose
product"*, and *"the geese should blend together where one's wing goes under the other, essentially
not being visible still offering the illusion of in flight and the L should be bigger"*.

- **The goose is not a drawing of ours.** `GOOSE_PATH` is the goose from `icons/Goose.tsx`, the
  upstream product's own mark, verbatim. Two rounds of hand-drawn birds were rejected. The lineage
  is the point — do not substitute a simpler bird.
- **The two geese overlap and MERGE, deliberately.** The back one's leading wing passes under the
  front one and is simply not drawn. An earlier version knocked a halo out between them; that gap is
  exactly what was rejected. **Do not reintroduce a mask.**
- The offset runs along the FLIGHT axis (back one low-left, front one high-right). Offsetting along
  the wing axis instead was tested and fuses the pair into one long diagonal streak with two heads.
  They are banked apart (-8 / +6) and differ slightly in size so it is not one stamp twice.
- `<LeanZero/>` (icons barrel) and `<LeanZeroGlyph/>` (ProjectLanding, carries `data-testid`) both
  render `<LeanZeroMarkContent/>`. Never hand-copy the paths into a third component.
- White on the accent square (sidebar brand, chat chip, landing EmptyState), LeanZero blue
  (`--color-action-solid`) on a plain surface, black in a menu-bar template. Colour is never baked
  into the mark.
- **Smallest honest size is 24px** — the original goose carries feather detail that fills in below
  that. The sidebar brand square is 32px with a 24px mark. If a new surface needs it smaller, make
  the surface bigger rather than shrinking the mark.
- The L is 36x53x14 in a 64 grid and is the dominant form. Its foot ends at x=40, which is what lets
  the back goose's lower wing clear it.
- App icon, tray templates and the Linux scalable icon are generated by
  `node scripts/build-brand-icons.mjs`, which READS `leanzeroMark.tsx` — so the shipped assets
  cannot drift from the mark the UI draws. Run it after any geometry change; never hand-edit
  `src/images/icon.*`. It fails loudly if the module changes shape rather than emitting a stale set.
