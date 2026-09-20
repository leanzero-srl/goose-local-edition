import type { McpToolInfo } from '../../types/mcpSetup';

export function McpCapabilities({ tools }: { tools: McpToolInfo[] }) {
  return (
    <details className="rounded-lg border border-border-primary p-3">
      <summary className="cursor-pointer text-sm font-medium">
        Available tools ({tools.length})
      </summary>
      <div className="mt-3 space-y-4">
        {tools.map((tool) => {
          const properties = tool.inputSchema.properties as
            | Record<string, { type?: string; description?: string }>
            | undefined;
          const required = Array.isArray(tool.inputSchema.required)
            ? tool.inputSchema.required
            : [];
          return (
            <section key={tool.name} className="min-w-0">
              <h4 className="break-words text-sm font-medium">{tool.name}</h4>
              <p className="mt-1 whitespace-pre-wrap text-xs leading-relaxed text-text-secondary">
                {tool.description}
              </p>
              {properties && (
                <details className="mt-2">
                  <summary className="cursor-pointer text-xs">Inputs this tool accepts</summary>
                  <dl className="mt-2 space-y-2 text-xs">
                    {Object.entries(properties).map(([name, schema]) => (
                      <div key={name}>
                        <dt className="font-medium">
                          {name} · {required.includes(name) ? 'Required' : 'Optional'}
                          {schema.type ? ` · ${schema.type}` : ''}
                        </dt>
                        {schema.description && (
                          <dd className="mt-1 text-text-secondary">{schema.description}</dd>
                        )}
                      </div>
                    ))}
                  </dl>
                </details>
              )}
            </section>
          );
        })}
      </div>
    </details>
  );
}
