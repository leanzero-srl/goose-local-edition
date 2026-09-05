import { SWARM_PROVIDER_ID } from '../../branding';

/** The cloud providers the panel can add nodes from — THE single mirror of the engine's CLOUD_DEFS
 *  (crates/goose-cli/src/commands/swarm.rs). `cli` = the `goose swarm cloud <cli>` name and the
 *  SwarmDevice.provider value; `registry` = the goose provider-registry id (CloudDef.registry),
 *  which is how configured-ness joins acpListProviderDetails; `seg` = the short label every
 *  surface shows for what serves a node. Every surface derives from this table — when the engine
 *  grows a cloud family, it is added HERE and nowhere else. No colour lives here: provider is
 *  text (a quiet Chip), node identity is the nodeHue ramp.
 *
 *  This module is DATA ONLY (no React) so the main process (utils/mainBrand.ts via
 *  leanzeroSelectorPolicy) can import the provider allow-list without pulling the renderer in. */
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
] as const;
export type CloudProviderDef = (typeof CLOUD_PROVIDERS)[number];

export const chipFor = (provider: string | null | undefined): CloudProviderDef | null =>
  CLOUD_PROVIDERS.find((c) => c.cli === provider) ?? null;

/** A node is local unless a cloud provider claims it: the LM Studio fleet, or the LeanZero MLX
 *  engine (SwarmDevice.engine === 'mlx-sidecar') — every row in the Nodes list is labelled by
 *  what serves it. */
export const LOCAL_CHIP = { seg: 'LM Studio' } as const;
export const MLX_CHIP = { seg: 'LeanZero MLX' } as const;

/** The registry ids of the swarm's cloud families — the ONLY cloud chat providers the Goose Swarm
 *  (local) edition offers. Derived from the table above so it cannot drift from it. */
export const LOCAL_EDITION_CLOUD_PROVIDER_IDS: readonly string[] = CLOUD_PROVIDERS.map(
  (c) => c.registry
);

/** Owner's rule (2026-09-05): "in the providers, we can only ever get the providers we have
 *  defined — which is only the defined cloud ones and Swarm. If you choose Swarm you tap into the
 *  nodes automatically." The exact set a user of the local edition can pick as a chat provider:
 *  the four cloud families by registry id, plus the Goose Swarm provider. Joined on registry id —
 *  never on the CLI family name, never on a name fragment. */
export const LOCAL_EDITION_PROVIDER_IDS: readonly string[] = [
  SWARM_PROVIDER_ID,
  ...LOCAL_EDITION_CLOUD_PROVIDER_IDS,
];
