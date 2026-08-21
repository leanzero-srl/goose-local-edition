import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';

// https://vitejs.dev/config
export default defineConfig({
  define: {
    'process.env.GOOSE_TUNNEL': JSON.stringify(process.env.GOOSE_TUNNEL !== 'no' && process.env.GOOSE_TUNNEL !== 'none'),
  },

  plugins: [tailwindcss()],

  // Vite caches a copy of @aaif/goose-sdk and doesn't notice when we rebuild it
  // locally, so it serves stale code until you clear node_modules/.vite by hand.
  // Excluding it makes Vite always read the latest ui/sdk/dist build.
  // Dev-server only — release builds ignore optimizeDeps.
  optimizeDeps: {
    exclude: ['@aaif/goose-sdk'],
    // Pin the dep scan to the app's real entry. Without this Vite globs **/*.html from the
    // project root, walks into out/ (packaged app + DMG staging from a release build — the
    // dmgstage/Applications SYMLINK even pulled in every app on the machine), and the scan
    // error KILLS the dev server: `pnpm start-gui` then boots Electron against a dead
    // localhost:5173 and shows a blank error page.
    entries: ['index.html'],
  },

  build: {
    target: 'esnext'
  },
});
