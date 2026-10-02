import { invoke, view } from '@forge/bridge';
import { widgetEdit } from '@forge/dashboards-bridge';
import { boot, h, mount } from '../../shared/dom';

interface Board {
  id: string;
  name: string;
}

// No onProductSave handler: the dashboard's Save stores the last updateConfig value as-is.
boot(async () => {
  const [ctx, boards] = await Promise.all([
    view.getContext() as Promise<{ extension?: { config?: { boardId?: string | number } } }>,
    invoke('boards') as Promise<Board[]>,
  ]);
  let selected = ctx.extension?.config?.boardId != null ? String(ctx.extension.config.boardId) : null;

  const options = boards.map((b) =>
    h('button', { type: 'button', class: 'board-option', 'data-testid': 'board-option', 'data-board-id': b.id, 'aria-pressed': String(b.id === selected) }, h('span', { class: 'board-name' }, b.name), h('span', { class: 'board-id' }, `#${b.id}`)),
  );
  const list = h('div', { class: 'board-list', role: 'group', 'aria-label': 'Scrum boards' }, ...options);
  list.addEventListener('click', async (e) => {
    const btn = (e.target as Element).closest('[data-testid="board-option"]') as HTMLButtonElement | null;
    if (!btn) return;
    selected = btn.dataset.boardId!;
    for (const o of options) o.setAttribute('aria-pressed', String(o.dataset.boardId === selected));
    await widgetEdit.updateConfig({ boardId: selected });
  });

  mount(
    h(
      'main',
      { class: 'edit' },
      h('h2', { class: 'edit-title' }, 'Scrum board'),
      h('p', { class: 'edit-help' }, 'The widget shows committed, added and removed points for this board’s active sprints.'),
      boards.length ? list : h('p', { class: 'state' }, 'No scrum boards are visible on this site.'),
    ),
  );
});
