import { invoke, view } from '@forge/bridge';

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Resolvers hand a long Jira Retry-After back instead of sleeping past their 25 s limit; the page
// waits it out and calls again.
export async function call(key, payload) {
  for (;;) {
    const res = await invoke(key, payload);
    if (res && res.rateLimited) {
      await wait(Math.max(1, Number(res.retryAfter) || 1) * 1000 + 50);
      continue;
    }
    return res;
  }
}

export async function boot(render) {
  await view.theme.enable();
  const context = await view.getContext();
  render(context);
}

export function formatInstant(iso, context) {
  const options = { year: 'numeric', month: 'short', day: '2-digit', hour: '2-digit', minute: '2-digit' };
  try {
    return new Intl.DateTimeFormat(context?.locale?.replace('_', '-') || undefined, { ...options, timeZone: context?.timezone || 'UTC', timeZoneName: 'short' }).format(new Date(iso));
  } catch {
    return new Intl.DateTimeFormat('en-GB', { ...options, timeZone: 'UTC', timeZoneName: 'short' }).format(new Date(iso));
  }
}
