import { describe, expect, it, beforeEach } from 'vitest';
import { act, fireEvent, render, renderHook, screen } from '@testing-library/react';
import { SectionFoldToggle, useSectionCollapsed } from './tree';

describe('sidebar section fold', () => {
  beforeEach(() => localStorage.clear());

  it('starts open, folds on toggle and remembers the fold per section', () => {
    const { result, unmount } = renderHook(() => useSectionCollapsed('projects'));
    expect(result.current[0]).toBe(false);
    act(() => result.current[1]());
    expect(result.current[0]).toBe(true);
    unmount();
    expect(renderHook(() => useSectionCollapsed('projects')).result.current[0]).toBe(true);
    expect(renderHook(() => useSectionCollapsed('benchmark')).result.current[0]).toBe(false);
  });

  it('the chevron announces its state and fires the toggle', () => {
    let toggled = 0;
    render(
      <SectionFoldToggle
        collapsed={false}
        onToggle={() => toggled++}
        label="Collapse Projects"
        testId="projects-fold"
      />
    );
    const button = screen.getByTestId('projects-fold');
    expect(button.getAttribute('aria-expanded')).toBe('true');
    expect(button.getAttribute('aria-label')).toBe('Collapse Projects');
    fireEvent.click(button);
    expect(toggled).toBe(1);
  });
});
