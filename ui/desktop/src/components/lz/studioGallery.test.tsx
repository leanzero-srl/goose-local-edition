import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import {
  Button,
  Chip,
  DataTable,
  EmptyState,
  KeyValue,
  PageHeader,
  Panel,
  SectionHeader,
  Segmented,
  StatusDot,
  Toolbar,
} from './index';
import { allClasses, assertStudioClean } from './assertStudioClean';
import { missingUtilities } from './compileStudioCss';

/**
 * Every primitive rendered once, in every register it has, and every class name it emitted
 * compiled through the REAL Tailwind pipeline against main.css. A class that produces no rule
 * is a silent no-op in the app (the `font-mono` case this system was born from); a banned
 * pattern anywhere in the tree fails the same test.
 */
function Gallery() {
  return (
    <div>
      <PageHeader
        eyebrow="Eyebrow"
        title="Title"
        subtitle="Subtitle"
        actions={<Button variant="primary">Go</Button>}
      />
      <SectionHeader title="Section" count={2} right={<Chip>quiet</Chip>} />
      <Segmented
        aria-label="seg"
        options={[
          { value: 'a', label: 'A' },
          { value: 'b', label: 'B', disabled: true },
        ]}
        value="a"
        onChange={() => {}}
      />
      <Segmented
        aria-label="seg-sm"
        size="sm"
        options={[{ value: 'a', label: 'A' }]}
        value="a"
        onChange={() => {}}
      />
      <Segmented
        as="buttons"
        aria-label="seg-buttons"
        disabled
        options={[
          { value: 'a', label: 'A', title: 'Locked', describedBy: 'why' },
          { value: 'b', label: 'B' },
        ]}
        value="a"
        onChange={() => {}}
      />
      <Segmented
        as="tabs"
        aria-label="seg-tabs"
        options={[
          { value: 'a', label: 'A', icon: <svg /> },
          { value: 'b', label: 'B' },
        ]}
        value="b"
        onChange={() => {}}
      />
      <Button variant="primary" icon={<svg />}>
        Primary
      </Button>
      <Button variant="secondary" size="sm">
        Secondary
      </Button>
      <Button variant="ghost" disabled>
        Ghost
      </Button>
      <Chip>quiet</Chip>
      <Chip tone="ok">ok</Chip>
      <Chip tone="warn">warn</Chip>
      <Chip tone="err">err</Chip>
      <Chip tone="stopped">stopped</Chip>
      <Chip tone="accent">accent</Chip>
      <Chip tone="secondary">secondary</Chip>
      {([1, 2, 3, 4, 5, 6] as const).map((n) => (
        <Chip key={n} node={n}>
          node {n}
        </Chip>
      ))}
      <StatusDot tone="ok" label="ok" />
      <StatusDot tone="warn" label="warn" live size={10} />
      <StatusDot tone="err" label="err" />
      <StatusDot tone="stopped" label="stopped" />
      <StatusDot tone="accent" label="accent" />
      <StatusDot tone="secondary" label="secondary" />
      {([1, 2, 3, 4, 5, 6] as const).map((n) => (
        <StatusDot key={n} node={n} label={`node ${n}`} />
      ))}
      <DataTable
        columns={[
          { key: 'n', header: 'Name', cell: (r: { id: string; n: number }) => r.id },
          { key: 'v', header: 'Value', cell: (r) => r.n, numeric: true },
          { key: 'c', header: 'Mid', cell: (r) => r.n, align: 'center' },
        ]}
        rows={[
          { id: 'x', n: 1 },
          { id: 'y', n: 2 },
        ]}
        rowKey={(r) => r.id}
        selectedKey="y"
        onRowClick={() => {}}
        rowAction={() => (
          <Button variant="ghost" size="sm">
            …
          </Button>
        )}
      />
      <DataTable
        columns={[{ key: 'n', header: 'Name', cell: (r: { id: string }) => r.id }]}
        rows={[]}
        rowKey={(r) => r.id}
        dense
        empty={<EmptyState title="Empty" />}
      />
      <EmptyState
        icon={<svg />}
        title="Nothing yet"
        body="Body"
        action={<Button variant="primary">Act</Button>}
      />
      <KeyValue
        items={[
          { key: 'a', label: 'Plain', value: 1 },
          { key: 'b', label: 'Err', value: 2, tone: 'err' },
          { key: 'c', label: 'Mono', value: 'abc', mono: true },
        ]}
      />
      <KeyValue dense items={[{ key: 'a', label: 'Dense', value: 1 }]} />
      <Toolbar
        search={{ value: 'q', onChange: () => {}, 'aria-label': 's' }}
        filters={<Chip>f</Chip>}
        actions={<Button>A</Button>}
      />
      <Panel title="Panel" count={1} headerRight={<Chip>r</Chip>}>
        body
      </Panel>
      <Panel padded={false}>flush</Panel>
    </div>
  );
}

describe('lz gallery — every primitive, every register', () => {
  it('carries no banned pattern anywhere in the tree', () => {
    const { container } = render(<Gallery />);
    assertStudioClean(container);
  });

  it('every class name any primitive emits compiles to a real rule against main.css', async () => {
    const { container } = render(<Gallery />);
    // lucide stamps its icons with its own (non-Tailwind) class names.
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(classes.length).toBeGreaterThan(80);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});
