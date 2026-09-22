import { cx } from '../lz';

/** One solid hue per cloud provider — brand-adjacent, saturated, identity only (never state). */
const PROVIDER_HUE: Readonly<Record<string, string>> = {
  openai: '#10a37f',
  anthropic: '#d97757',
  google: '#4285f4',
  aws_bedrock: '#ff9900',
  azure_openai: '#0078d4',
  custom_deepseek: '#4d6bfe',
  mistral: '#ff7000',
  minimax: '#e4002b',
  moonshot: '#7c3aed',
  ollama_cloud: '#0ea5e9',
  openrouter: '#6467f2',
  alibaba: '#615ced',
  xai: '#1d4ed8',
  zai: '#16a34a',
};

const SIZE = { sm: 'size-8 text-[13px]', md: 'size-10 text-[16px]' } as const;

export function providerHue(providerId: string): string {
  return PROVIDER_HUE[providerId] ?? '#64748b';
}

/** A solid colour tile carrying the provider's initial — the visual anchor of every provider row. */
export function ProviderTile({
  providerId,
  label,
  size = 'sm',
}: {
  providerId: string;
  label: string;
  size?: keyof typeof SIZE;
}) {
  return (
    <span
      aria-hidden
      className={cx(
        'inline-flex shrink-0 items-center justify-center rounded-lz-control font-lz-semibold text-white',
        SIZE[size]
      )}
      style={{ backgroundColor: providerHue(providerId) }}
    >
      {label.trim().charAt(0).toUpperCase()}
    </span>
  );
}
