import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import type { HistoryItem } from '../../acp/controllerTypes';

type Position = { id: string | null; offset: number; top: number; height: number };
const atBottom = (element: HTMLElement) =>
  element.scrollHeight - element.clientHeight - element.scrollTop <= 2;

export function useControllerTranscriptScroll(
  identity: string,
  ready: boolean,
  items: HistoryItem[] | undefined,
  loadingOlder: boolean
) {
  const transcript = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const selected = useRef<string | null>(null);
  const follow = useRef(true);
  const position = useRef<Position | null>(null);
  const prepending = useRef(false);
  const [showLatest, setShowLatest] = useState(false);

  const remember = useCallback(() => {
    const element = transcript.current;
    if (!element) return;
    const top = element.getBoundingClientRect().top;
    const anchor = [...element.querySelectorAll<HTMLElement>('[data-controller-message-id]')].find(
      (row) => row.getBoundingClientRect().bottom > top
    );
    position.current = {
      id: anchor?.dataset.controllerMessageId ?? null,
      offset: anchor ? anchor.getBoundingClientRect().top - top : 0,
      top: element.scrollTop,
      height: element.scrollHeight,
    };
  }, []);

  const synchronize = useCallback(() => {
    const element = transcript.current;
    if (!element || !ready) return;
    if (selected.current !== identity) {
      selected.current = identity;
      follow.current = true;
      position.current = null;
      prepending.current = false;
    }
    if (follow.current && !prepending.current) {
      element.scrollTop = element.scrollHeight;
    } else if (position.current) {
      const previous = position.current;
      const anchor = [
        ...element.querySelectorAll<HTMLElement>('[data-controller-message-id]'),
      ].find((row) => row.dataset.controllerMessageId === previous.id);
      if (anchor) {
        element.scrollTop +=
          anchor.getBoundingClientRect().top -
          element.getBoundingClientRect().top -
          previous.offset;
      } else {
        element.scrollTop =
          previous.top + (prepending.current ? element.scrollHeight - previous.height : 0);
      }
    }
    if (!loadingOlder) prepending.current = false;
    setShowLatest(!atBottom(element));
    remember();
  }, [identity, ready, loadingOlder, remember]);

  useLayoutEffect(synchronize, [synchronize, items]);
  useLayoutEffect(() => {
    if (!content.current || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(synchronize);
    observer.observe(content.current);
    if (transcript.current) observer.observe(transcript.current);
    return () => observer.disconnect();
  }, [synchronize]);

  const onScroll = () => {
    const element = transcript.current;
    if (!element) return;
    follow.current = atBottom(element);
    setShowLatest(!follow.current);
    remember();
  };
  const jumpLatest = () => {
    const element = transcript.current;
    if (!element) return;
    follow.current = true;
    prepending.current = false;
    element.scrollTop = element.scrollHeight;
    setShowLatest(false);
    remember();
  };
  const beforeOlder = () => {
    follow.current = false;
    prepending.current = true;
    remember();
  };
  return { transcript, content, onScroll, jumpLatest, beforeOlder, showLatest };
}
