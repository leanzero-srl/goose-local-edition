/** The cloud providers the panel can add nodes from — THE single mirror of the engine's CLOUD_DEFS
 *  (crates/goose-cli/src/commands/swarm.rs). `cli` = the `goose swarm cloud <cli>` name and the
 *  SwarmDevice.provider value; `registry` = the goose provider-registry id (CloudDef.registry),
 *  which is how configured-ness joins acpListProviderDetails; `seg` = the short label every
 *  surface shows for what serves a node. Swarm node surfaces derive from this table — when the engine
 *  grows a cloud family, it is added HERE and nowhere else. No colour lives here: provider is
 *  text (a quiet Chip), node identity is the nodeHue ramp.
 *
 *  This module describes swarm node adapters only. Provider configuration uses the engine registry. */
export const CLOUD_PROVIDERS = [
  {
    seg: 'Bedrock',
    cli: 'bedrock',
    registry: 'aws_bedrock',
    label: 'Amazon Bedrock',
    keyPlaceholder: 'Bedrock API key (ABSK…)',
    region: true,
  },
  {
    seg: 'Z.ai',
    cli: 'zai',
    registry: 'zai',
    label: 'Z.ai',
    keyPlaceholder: 'Z.ai API key',
    region: false,
  },
  {
    seg: 'Gemini',
    cli: 'google',
    registry: 'google',
    label: 'Google Gemini',
    keyPlaceholder: 'Gemini API key (AIza…)',
    region: false,
  },
  {
    seg: 'DeepSeek',
    cli: 'deepseek',
    registry: 'custom_deepseek',
    label: 'DeepSeek',
    keyPlaceholder: 'DeepSeek API key (sk-…)',
    region: false,
  },
  {
    seg: 'Azure Foundry',
    cli: 'azure_openai',
    registry: 'azure_openai',
    label: 'Azure Foundry',
    keyPlaceholder: 'Azure Foundry API key',
    region: false,
  },
  {
    seg: 'OpenAI',
    cli: 'openai',
    registry: 'openai',
    label: 'OpenAI',
    keyPlaceholder: 'OpenAI API key',
    region: false,
  },
  {
    seg: 'Claude',
    cli: 'anthropic',
    registry: 'anthropic',
    label: 'Claude',
    keyPlaceholder: 'Claude API key',
    region: false,
  },
  {
    seg: 'Qwen',
    cli: 'alibaba',
    registry: 'alibaba',
    label: 'Qwen',
    keyPlaceholder: 'Qwen API key',
    region: false,
  },
  {
    seg: 'OpenRouter',
    cli: 'openrouter',
    registry: 'openrouter',
    label: 'OpenRouter',
    keyPlaceholder: 'OpenRouter API key',
    region: false,
  },
  {
    seg: 'Ollama Cloud',
    cli: 'ollama_cloud',
    registry: 'ollama_cloud',
    label: 'Ollama Cloud',
    keyPlaceholder: 'Ollama Cloud API key',
    region: false,
  },
  {
    seg: 'MiniMax',
    cli: 'minimax',
    registry: 'minimax',
    label: 'MiniMax',
    keyPlaceholder: 'MiniMax API key',
    region: false,
  },
  {
    seg: 'Mistral AI',
    cli: 'mistral',
    registry: 'mistral',
    label: 'Mistral AI',
    keyPlaceholder: 'Mistral AI API key',
    region: false,
  },
  {
    seg: 'xAI',
    cli: 'xai',
    registry: 'xai',
    label: 'xAI',
    keyPlaceholder: 'xAI API key',
    region: false,
  },
  {
    seg: 'Moonshot',
    cli: 'moonshot',
    registry: 'moonshot',
    label: 'Moonshot',
    keyPlaceholder: 'Moonshot API key',
    region: false,
  },
] as const;
export type CloudProviderDef = (typeof CLOUD_PROVIDERS)[number];

export const chipFor = (provider: string | null | undefined): CloudProviderDef | null =>
  CLOUD_PROVIDERS.find((c) => c.cli === provider) ?? null;

/** A node is local unless a cloud provider claims it: the LM Studio fleet, or the LeanZero MLX
 *  engine (SwarmDevice.engine === 'mlx-sidecar') — every row in the Nodes list is labelled by
 *  what serves it. */
export const LOCAL_CHIP = { seg: 'LM Studio' } as const;
export const MLX_CHIP = { seg: 'LeanZero MLX' } as const;
