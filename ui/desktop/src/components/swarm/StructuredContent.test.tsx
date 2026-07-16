import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import StructuredContent, { classifyContent } from './StructuredContent';
import { IntlTestWrapper } from '../../i18n/test-utils';

// The bug, exactly as Mihai screenshotted it: the plan skeleton arrived as raw JSON, and markdown both
// reflowed it into a wall AND read `__init__` as bold syntax — so the panel displayed **init**.py, a
// filename that does not exist, in the view whose whole job is saying which files got written.
describe('StructuredContent', () => {
  const PLAN =
    '{"subtasks":[{"id":"init","description":"Package init for kanban","difficulty":"easy","model":"qwen/qwen3.6-35b-a3b","depends_on":[],"files":["kanban/__init__.py","kanban/__main__.py"]},{"id":"api","description":"Flask app with REST routes","difficulty":"hard","depends_on":["db"],"files":["kanban/api.py"]}],"integration":"python3 -m pytest tests/ -v"}';

  it('never corrupts a dunder filename into bold', () => {
    const { container } = render(<StructuredContent content={PLAN} />);
    expect(container.querySelector('strong'), 'markdown must not touch a payload').toBeNull();
    expect(container.textContent).toContain('kanban/__init__.py');
    expect(container.textContent).toContain('kanban/__main__.py');
  });

  it('renders the plan as a task list, not as JSON soup', () => {
    const { container } = render(<StructuredContent content={PLAN} />);
    const txt = container.textContent ?? '';
    expect(txt).toContain('init');
    expect(txt).toContain('Package init for kanban');
    expect(txt).toContain('after db');            // deps read as English
    expect(txt).not.toContain('"subtasks"');      // no raw JSON keys leak through
    expect(txt).not.toContain('depends_on');
  });

  it('pretty-prints any OTHER json payload in a mono block instead of reflowing it', () => {
    const { container } = render(<StructuredContent content='{"b":2,"a":{"c":[1,2]}}' />);
    const pre = container.querySelector('pre');
    expect(pre).not.toBeNull();
    expect(pre?.textContent).toContain('\n');     // actually pretty-printed
    expect(container.querySelector('strong')).toBeNull();
  });

  it('leaves real prose on the markdown path (bold still works where it belongs)', () => {
    const { container } = render(
      <IntlTestWrapper>
        <StructuredContent content={'Wrote **the parser** and ran `pytest`.'} />
      </IntlTestWrapper>
    );
    expect(container.querySelector('strong')?.textContent).toBe('the parser');
    expect(container.querySelector('code')?.textContent).toBe('pytest');
  });

  it('classifies without throwing on malformed/edge input', () => {
    expect(classifyContent('{not json at all').kind).toBe('prose');
    expect(classifyContent('').kind).toBe('prose');
    expect(classifyContent('null').kind).toBe('prose');
    expect(classifyContent('{"subtasks":[]}').kind).toBe('json');      // empty = not a plan
    expect(classifyContent('{"subtasks":[{"no":"id"}]}').kind).toBe('json'); // no id = not a plan
    expect(classifyContent('Just prose about {braces}.').kind).toBe('prose');
  });
});
