import type { InitializeResponse } from '@agentclientprotocol/sdk';
import { getAcpInitializeResponse } from './acpConnection';

export interface AcpFeatureCapabilities {
  localInference: boolean;
  mlxEngine: boolean;
  leanzeroLink: boolean;
}

export async function getAcpFeatureCapabilities(): Promise<AcpFeatureCapabilities> {
  const initializeResponse = await getAcpInitializeResponse();

  return {
    localInference: hasLocalInferenceCapability(initializeResponse),
    mlxEngine: hasGooseCapability(initializeResponse, 'mlxEngine'),
    leanzeroLink: hasGooseCapability(initializeResponse, 'leanzeroLink'),
  };
}

export function hasLocalInferenceCapability(
  initializeResponse: Pick<InitializeResponse, 'agentCapabilities'>
): boolean {
  return hasGooseCapability(initializeResponse, 'localInference');
}

export function hasGooseCapability(
  initializeResponse: Pick<InitializeResponse, 'agentCapabilities'>,
  capability: string
): boolean {
  const agentCapabilities = initializeResponse.agentCapabilities;
  if (!agentCapabilities) {
    return false;
  }

  const meta = agentCapabilities._meta;
  if (!isRecord(meta)) {
    return false;
  }

  const goose = meta.goose;
  if (!isRecord(goose)) {
    return false;
  }

  return capability in goose;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
