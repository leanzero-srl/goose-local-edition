/**
 * The Mac a dropped turn was served by, stored WITH the drop (Q-62): the notice first resolves the
 * name while the route still names that Mac; a later route change (another Mac, or none) must not
 * rename it to "The linked Mac". Kept per message id in this window and in localStorage, so a
 * relaunch keeps it too.
 */
const STORAGE_KEY = 'goose.linkDropNames';

/**
 * The names kept, newest last. // ratio: the last 200 dropped turns — a window of chats far past
 * any one session's scroll-back; older ones fall back to whatever the route names then.
 */
const KEEP = 200;

let names: Map<string, string> | null = null;

function load(): Map<string, string> {
  if (names) return names;
  names = new Map();
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    if (Array.isArray(parsed)) {
      for (const entry of parsed) {
        if (Array.isArray(entry) && typeof entry[0] === 'string' && typeof entry[1] === 'string') {
          names.set(entry[0], entry[1]);
        }
      }
    }
  } catch {
    // Unreadable storage keeps this window's names only; the notice still names what it knows.
  }
  return names;
}

export function storedDropName(messageId: string): string | null {
  return load().get(messageId) ?? null;
}

export function rememberDropName(messageId: string, name: string): void {
  const map = load();
  if (map.get(messageId) === name) return;
  map.delete(messageId);
  map.set(messageId, name);
  while (map.size > KEEP) {
    const oldest = map.keys().next().value;
    if (oldest === undefined) break;
    map.delete(oldest);
  }
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify([...map.entries()]));
  } catch {
    // Storage full or unavailable: the name is kept for this window.
  }
}

/** Tests only: forget every stored name. */
export function resetDropNamesForTests(): void {
  names = new Map();
  try {
    window.localStorage.removeItem(STORAGE_KEY);
  } catch {
    // nothing stored
  }
}
