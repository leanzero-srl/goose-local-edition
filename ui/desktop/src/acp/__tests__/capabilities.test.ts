import type { InitializeResponse } from '@agentclientprotocol/sdk';
import { describe, expect, it } from 'vitest';
import { hasGooseCapability, hasLocalInferenceCapability } from '../capabilities';

function initializeResponseWithMeta(meta?: unknown): Pick<InitializeResponse, 'agentCapabilities'> {
  return {
    agentCapabilities: {
      _meta: meta,
    },
  } as Pick<InitializeResponse, 'agentCapabilities'>;
}

describe('ACP capabilities', () => {
  it('detects local inference support from Goose metadata', () => {
    expect(
      hasLocalInferenceCapability(
        initializeResponseWithMeta({
          goose: {
            localInference: {},
          },
        })
      )
    ).toBe(true);
  });

  it('treats missing local inference metadata as unsupported', () => {
    expect(hasLocalInferenceCapability(initializeResponseWithMeta())).toBe(false);
    expect(hasLocalInferenceCapability(initializeResponseWithMeta({}))).toBe(false);
    expect(hasLocalInferenceCapability(initializeResponseWithMeta({ goose: {} }))).toBe(false);
  });

  it('ignores malformed Goose metadata', () => {
    expect(hasLocalInferenceCapability(initializeResponseWithMeta({ goose: true }))).toBe(false);
    expect(hasLocalInferenceCapability(initializeResponseWithMeta({ goose: null }))).toBe(false);
  });

  it('detects the MLX engine capability from Goose metadata', () => {
    expect(
      hasGooseCapability(initializeResponseWithMeta({ goose: { mlxEngine: {} } }), 'mlxEngine')
    ).toBe(true);
    expect(hasGooseCapability(initializeResponseWithMeta({ goose: {} }), 'mlxEngine')).toBe(false);
    expect(hasGooseCapability(initializeResponseWithMeta(), 'mlxEngine')).toBe(false);
  });
});
