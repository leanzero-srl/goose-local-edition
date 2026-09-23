import { useEffect, type RefObject } from 'react';

/**
 * ⌘F / Ctrl+F focuses (and selects) a view's own search field. Both routes are heard: the app
 * menu's Find… item holds the accelerator and sends `find-command` over IPC, and a plain keydown
 * reaches the renderer when no menu claims it (tests, a menu without the item).
 */
export function useFindShortcut(inputRef: RefObject<HTMLInputElement | null>): void {
  useEffect(() => {
    const focus = () => {
      const input = inputRef.current;
      if (!input) return;
      input.focus();
      input.select();
    };
    const onKey = (e: KeyboardEvent) => {
      const isMac = window.electron?.platform === 'darwin';
      if ((isMac ? e.metaKey : e.ctrlKey) && !e.shiftKey && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        focus();
      }
    };
    window.addEventListener('keydown', onKey);
    window.electron?.on('find-command', focus);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.electron?.off('find-command', focus);
    };
  }, [inputRef]);
}
