import { bootFirstPaint, el, metrics } from '../../shared/boot';
import '../../shared/base.css';
import './widget.css';

const boardOf = (context) => context?.extension?.config?.boardId;

bootFirstPaint({
  request: (context) => {
    const boardId = boardOf(context);
    return boardId === undefined || boardId === null || boardId === '' ? null : { key: 'widget', payload: { boardId: String(boardId) } };
  },
  paint: (root, data) => {
    const widget = el('div', { 'data-testid': 'scope-widget', class: 'widget' });
    if (!data || data.needsConfig) widget.append(el('p', { 'data-testid': 'needs-config', class: 'message' }, "Choose a scrum board in this widget's settings to see its sprint scope."));
    else if (data.ok === false) widget.append(el('p', { class: 'message error', role: 'alert' }, `Could not load sprint scope: ${data.error}`));
    else if (!data.sprints.length) widget.append(el('p', { class: 'message' }, 'This board has no active sprint.'));
    else {
      const sprints = data.sprints.map((s) => el('section', { class: 'sprint', 'data-testid': 'sprint', 'data-sprint-id': s.id }, el('h3', { class: 'sprint-name' }, s.name), metrics(s.text)));
      widget.append(el('div', { class: 'sprints' }, ...sprints));
    }
    root.replaceChildren(widget);
  },
});
