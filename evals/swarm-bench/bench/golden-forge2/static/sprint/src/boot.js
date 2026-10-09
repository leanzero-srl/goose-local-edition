import { bootFirstPaint, el, metrics } from '../../shared/boot';
import '../../shared/base.css';
import './sprint.css';

bootFirstPaint({
  request: () => ({ key: 'sprintLedger', payload: undefined }),
  paint: (root, data) => {
    const box = el('div', { 'data-testid': 'sprint-ledger', class: 'sprint-action' });
    if (data?.notStarted) box.append(el('p', { 'data-testid': 'not-started', class: 'message' }, `${data.sprint.name} has not started yet, so nothing has entered or left it.`));
    else if (!data || data.error) box.append(el('h2', {}, 'Scope ledger'), el('p', { class: 'message error', role: 'alert' }, data?.error ?? 'The sprint could not be loaded.'));
    else {
      const hidden = el('p', { class: 'hidden-line' }, el('span', { 'data-testid': 'hidden-count', class: 'hidden-count' }, String(data.hiddenCount)), ` ${data.hiddenCount === 1 ? 'change is' : 'changes are'} on issues you cannot browse and not listed.`);
      box.append(el('h2', {}, `Scope ledger · ${data.sprint.name}`), metrics(data.text), hidden);
    }
    root.replaceChildren(box);
  },
});
