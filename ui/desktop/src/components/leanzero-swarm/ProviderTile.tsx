import { Server } from 'lucide-react';
import { cx } from '../lz';
import openai from './provider-logos/openai.svg?raw';
import anthropic from './provider-logos/anthropic.svg?raw';
import google from './provider-logos/google.svg?raw';
import awsBedrock from './provider-logos/aws_bedrock.svg?raw';
import azureOpenai from './provider-logos/azure_openai.svg?raw';
import deepseek from './provider-logos/custom_deepseek.svg?raw';
import mistral from './provider-logos/mistral.svg?raw';
import ollamaCloud from './provider-logos/ollama_cloud.svg?raw';
import alibaba from './provider-logos/alibaba.svg?raw';
import openrouter from './provider-logos/openrouter.svg?raw';
import minimax from './provider-logos/minimax.svg?raw';
import moonshot from './provider-logos/moonshot.svg?raw';
import xaiPng from './provider-logos/xai.png';

/** The tile id every OpenAI-compatible endpoint wears: they are one family (any server speaking the
 *  OpenAI API), told apart by the name the person gave each. */
export const OPENAI_COMPATIBLE_TILE = 'openai_compatible';

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
  [OPENAI_COMPATIBLE_TILE]: '#c026d3',
};

/** The provider's own mark (simple-icons, monochrome) drawn in white on its tile. Z.ai has no
 *  published mark in that set and keeps its initial; xAI ships as a PNG. */
const PROVIDER_MARK: Readonly<Record<string, string>> = {
  openai,
  anthropic,
  google,
  aws_bedrock: awsBedrock,
  azure_openai: azureOpenai,
  custom_deepseek: deepseek,
  mistral,
  ollama_cloud: ollamaCloud,
  alibaba,
  openrouter,
  minimax,
  moonshot,
};

const SIZE = { sm: 'size-8 text-[13px] p-1.5', md: 'size-10 text-[16px] p-2' } as const;

export function providerHue(providerId: string): string {
  return PROVIDER_HUE[providerId] ?? '#64748b';
}

/** A solid colour tile carrying the provider's real mark — the visual anchor of every provider row. */
export function ProviderTile({
  providerId,
  label,
  size = 'sm',
}: {
  providerId: string;
  label: string;
  size?: keyof typeof SIZE;
}) {
  const mark = PROVIDER_MARK[providerId];
  return (
    <span
      aria-hidden
      data-testid={`provider-tile-${providerId}`}
      data-mark={
        mark
          ? 'svg'
          : providerId === 'xai'
            ? 'png'
            : providerId === OPENAI_COMPATIBLE_TILE
              ? 'icon'
              : 'initial'
      }
      className={cx(
        'inline-flex shrink-0 items-center justify-center rounded-lz-control font-lz-semibold text-white',
        '[&_svg]:size-full [&_svg]:fill-current',
        SIZE[size]
      )}
      style={{ backgroundColor: providerHue(providerId) }}
    >
      {mark ? (
        <span className="contents" dangerouslySetInnerHTML={{ __html: mark }} />
      ) : providerId === 'xai' ? (
        <img src={xaiPng} alt="" className="size-full invert" />
      ) : providerId === OPENAI_COMPATIBLE_TILE ? (
        <Server className="size-full !fill-none" />
      ) : (
        label.trim().charAt(0).toUpperCase()
      )}
    </span>
  );
}
