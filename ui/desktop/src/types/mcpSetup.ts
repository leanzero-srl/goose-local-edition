export interface McpToolInfo {
  name: string;
  description?: string;
  inputSchema: Record<string, unknown>;
}
export interface McpSetupResult {
  tools: McpToolInfo[];
  instructions?: string;
  savedFile?: string;
}
