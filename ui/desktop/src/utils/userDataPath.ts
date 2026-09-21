import path from 'node:path';
import { app } from 'electron';

// The product is "Goose Swarm" (3.0.8), and Electron derives userData from the product name —
// which would silently start every updated install with an empty Application Support dir:
// settings.json, projects.json, recent dirs, window state, the renderer's localStorage and
// the benchmark work root all live there. Pin it to the directory every earlier release used.
// Imported FIRST by main.ts: other modules capture app.getPath('userData') at import time.
app.setPath('userData', path.join(app.getPath('appData'), 'Goose'));
