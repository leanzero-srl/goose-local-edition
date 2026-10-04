import type { ModelFieldDto, ModelFieldSourceDto } from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

export type { ModelFieldDto, ModelFieldSourceDto };

/** A saved custom-field value: a select's option or a number. */
export type ModelFieldValue = string | number;
export type ModelFieldValues = Record<string, ModelFieldValue>;

export interface ModelFieldsListing {
  fields: ModelFieldDto[];
  source: ModelFieldSourceDto;
  values: ModelFieldValues;
}

function onlyFieldValues(values: Record<string, unknown>): ModelFieldValues {
  const clean: ModelFieldValues = {};
  for (const [id, value] of Object.entries(values)) {
    if (typeof value === 'string' || typeof value === 'number') clean[id] = value;
  }
  return clean;
}

/** The custom fields `modelId` takes on `providerId`, read from the provider's own model metadata
 *  (OpenRouter's listing) or goose's effort mapping, with the values saved for it. */
export async function acpListModelFields(
  providerId: string,
  modelId: string
): Promise<ModelFieldsListing> {
  const client = await getAcpClient();
  const response = await client.goose.providersModelFieldsList_unstable({ providerId, modelId });
  return {
    fields: response.fields,
    source: response.source,
    values: onlyFieldValues(response.values),
  };
}

/** Replaces the saved values; a field left out runs on the model's own default. The engine checks
 *  every value against the field the model declares and answers with what it saved. */
export async function acpSaveModelFields(
  providerId: string,
  modelId: string,
  values: ModelFieldValues
): Promise<ModelFieldValues> {
  const client = await getAcpClient();
  const response = await client.goose.providersModelFieldsSave_unstable({
    providerId,
    modelId,
    values,
  });
  return onlyFieldValues(response.values);
}
