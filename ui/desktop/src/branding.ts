// LeanZero / Goose Swarm branding constants. Single source for the
// name, links and provider display used across the Goose Swarm (local edition) UI.
//
// NAMING (2026-09-05, owner): the PRODUCT is "Goose Swarm" — that is the official name, and every
// rendered label says Swarm (a two-day "Flock" detour was reverted). The INTERNAL identifiers are
// `swarm` too: the config key, the `.swarm/` run directory, the IPC channels, the crate and file
// names and the GOOSE_SWARM_* env vars — display names live here; ids never move.
export const LEANZERO_NAME = 'LeanZero';
export const LEANZERO_WEBSITE_URL = 'https://leanzero.net/overview';
export const LEANZERO_DOCS_URL = 'https://leanzero.net/portfolio/goose-local-edition';

// The internal provider id stays 'swarm' (config key, tab value, CLI alias);
// this is only the user-facing display name.
export const SWARM_PROVIDER_ID = 'swarm';
export const SWARM_DISPLAY_NAME = 'Goose Swarm';

// Where this app's bug reports and feature requests go: OUR fork, never the parent goose repo
// (queued fix #8 — Report-a-Bug/Request-a-Feature/Diagnostics used to file against the parent's
// tracker, sending users of this build to a project that does not ship it). The fork carries the
// same .github/ISSUE_TEMPLATE files, so the template query params keep working.
export const LEANZERO_REPO_SLUG = 'leanzero-srl/goose-local-edition';
export const LEANZERO_ISSUES_NEW_URL = `https://github.com/${LEANZERO_REPO_SLUG}/issues/new`;

// Report a problem (Q-192): where a report goes, and the community door beside it.
// The inbox the site's contact form delivers to (Amplify EMAIL_TO, read 2026-09-27) and the address
// leanzero.net/contact prints.
export const LEANZERO_SUPPORT_EMAIL = 'office@leanzero.net';
// leanzero.net's own contact endpoint (src/app/api/contact/route.ts in the website repo): the
// recipient is fixed server-side, so this app can only ever reach office@ through it.
export const LEANZERO_CONTACT_API_URL = 'https://leanzero.net/api/contact';
// The invite leanzero.net links from its footer and /contact (NEXT_PUBLIC_DISCORD_INVITE_URL on the
// site's Amplify app); Discord's invite API answered guild "LeanZero Atlassian", no expiry, on
// 2026-09-27. The blog .env's 2BpFpxnEKw is EXPIRED — never that one.
export const LEANZERO_DISCORD_INVITE_URL = 'https://discord.gg/RvYbd9qEUT';
