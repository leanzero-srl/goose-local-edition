/**
 * A linked Mac that said it is going away on purpose — Link's `LeaveReason::describe()` words
 * (crates/leanzero-link/src/wire.rs: "Work's Mac Studio quit goose", "… is restarting goose"),
 * carried in the route's reason and in the relay's drop error. The engine goes with the app (Q-34),
 * so an answer in flight is gone. Pure: main's tray and the renderer read it alike.
 */
export type LeaveCause = 'quit' | 'restart';

export function leaveCause(text: string | null | undefined): LeaveCause | null {
  if (!text) return null;
  if (/\bis restarting goose\b/.test(text)) return 'restart';
  if (/\bquit goose\b/.test(text)) return 'quit';
  return null;
}
