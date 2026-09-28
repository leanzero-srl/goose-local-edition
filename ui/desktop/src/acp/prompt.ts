import type { ContentBlock, PromptResponse } from '@agentclientprotocol/sdk';
import type { SteerSessionRequest_unstable, SteerSessionResponse_unstable } from '@aaif/goose-sdk';
import type { Message } from '../types/message';
import { getAcpClient } from './acpConnection';

/**
 * The prompt's ACP `_meta`. A loop tick carries `goose.loopTick`, which goosed matches against
 * the runner's open offer; a needs-you card's answer carries `goose.needsYouAnswers`
 * ({@link needsYouAnswersMeta}); anything else it reads as an ordinary user prompt.
 */
export type AcpPromptMeta = Record<string, unknown>;

/**
 * Q-344: marks a message as the answer to needs-you questions given on their cards. Without it,
 * goosed reads the message as typed and closes every question still open in the chat as superseded
 * (Q-298) — the ones the person has not got to yet included. goosed honours the mark once, and only
 * for questions answered on the card whose answers the message carries.
 */
export function needsYouAnswersMeta(itemIds: readonly string[]): AcpPromptMeta {
  return { goose: { needsYouAnswers: [...itemIds] } };
}

/**
 * Q-381: "Answer on {next} for now" — THIS prompt's reply answers on `node`, a node of the chat's
 * own set after its lead, without changing the set; the next prompt carries no mark and goes back
 * to the lead. goosed refuses the turn by name when the route cannot honour it.
 */
export function answerOnMeta(node: string): AcpPromptMeta {
  return { goose: { answerOn: { node } } };
}

/** One prompt `_meta` carrying every `goose` mark given; undefined when none is. */
export function promptMeta(...metas: (AcpPromptMeta | undefined)[]): AcpPromptMeta | undefined {
  const given = metas.filter((m): m is AcpPromptMeta => m != null);
  if (given.length === 0) return undefined;
  return {
    goose: Object.assign({}, ...given.map((m) => m.goose as Record<string, unknown>)),
  };
}

export async function acpPromptSession(
  sessionId: string,
  message: Message,
  meta?: AcpPromptMeta
): Promise<PromptResponse> {
  const client = await getAcpClient();
  return client.prompt({
    sessionId,
    prompt: messageToAcpPromptContent(message),
    ...(meta ? { _meta: meta } : {}),
  });
}

export async function acpCancelPrompt(sessionId: string): Promise<void> {
  const client = await getAcpClient();
  await client.cancel({ sessionId });
}

export async function acpSteerSession(
  sessionId: string,
  message: Message,
  expectedRunId: string
): Promise<SteerSessionResponse_unstable> {
  const client = await getAcpClient();
  return client.goose.sessionSteer_unstable({
    sessionId,
    expectedRunId,
    prompt: messageToAcpPromptContent(message) as unknown as SteerSessionRequest_unstable['prompt'],
  });
}

export function messageToAcpPromptContent(message: Message): ContentBlock[] {
  const prompt: ContentBlock[] = [];

  for (const content of message.content) {
    switch (content.type) {
      case 'text':
        if (content.text.trim()) {
          prompt.push({
            type: 'text',
            text: content.text,
          });
        }
        break;
      case 'image':
        prompt.push({
          type: 'image',
          data: content.data,
          mimeType: content.mimeType,
        });
        break;
    }
  }

  return prompt;
}
