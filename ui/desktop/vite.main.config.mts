import { defineConfig } from 'vite';

// https://vitejs.dev/config
export default defineConfig({
  define: {
    // UPDATE FEED SEVERED FROM THE PARENT (owner decision, Goose Swarm pass A): these defines are
    // baked into the built main bundle and are what the packaged app's updater actually uses —
    // they used to default to the PARENT goose org's repo, silently overriding the leanzero
    // defaults in src/utils/autoUpdater.ts / githubUpdater.ts. Goose Swarm owns its own versioning
    // (2.x) and release line; the parent repo must never be queried for updates.
    // updateFeedSevered.test.ts pins this file to the leanzero release line.
    'process.env.GITHUB_OWNER': JSON.stringify(process.env.GITHUB_OWNER || 'leanzero-srl'),
    'process.env.GITHUB_REPO': JSON.stringify(process.env.GITHUB_REPO || 'goose-local-edition'),
    'process.env.GOOSE_BUNDLE_NAME': JSON.stringify(process.env.GOOSE_BUNDLE_NAME || 'Goose'),
  },
});
