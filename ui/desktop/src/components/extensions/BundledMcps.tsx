import { useEffect, useState } from 'react';
import { Plug } from 'lucide-react';
import { Button, Panel } from '../lz';
import { useConfig } from '../ConfigContext';
import { activateExtensionDefault } from '../settings/extensions';

type BundledMcp = Awaited<ReturnType<typeof window.electron.bundledMcps>>[number];
export function BundledMcps() {
  const { addExtension, extensionsList } = useConfig();
  const [entries, setEntries] = useState<BundledMcp[]>([]);
  const [error, setError] = useState('');
  useEffect(() => {
    window.electron
      .bundledMcps()
      .then(setEntries)
      .catch((e: unknown) => setError(String(e)));
  }, []);
  return (
    <Panel title="Included with Goose Swarm">
      <p className="mb-4 text-sm text-lz-ink-2">
        LeanZero MCP servers ship inside the app. Enable them here, then use their settings below to
        configure credentials and access.
      </p>
      {error && (
        <p role="alert" className="text-lz-err">
          Bundled servers unavailable: {error}
        </p>
      )}
      <div className="grid gap-4 md:grid-cols-2">
        {entries.map((entry) => {
          const installed = extensionsList.some((item) => item.name === entry.name);
          return (
            <div key={entry.name} className="rounded-lg border border-lz-border p-4">
              <h2 className="font-semibold">{entry.name}</h2>
              <p className="my-3 text-sm text-lz-ink-2">{entry.description}</p>
              <Button
                icon={<Plug />}
                disabled={installed}
                onClick={async () => {
                  await activateExtensionDefault({
                    addToConfig: addExtension,
                    extensionConfig: entry,
                  });
                }}
              >
                {installed ? 'Added — manage below' : 'Enable'}
              </Button>
            </div>
          );
        })}
      </div>
    </Panel>
  );
}
