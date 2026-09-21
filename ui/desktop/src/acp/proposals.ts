import type {
  AnswerMemoryProposalResponse_unstable,
  MemoryProposalDecision,
  MemoryProposalDto,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

export type { MemoryProposalDto };

/** Every proposal for a session: the end-of-turn assessment's and `propose_knowledge`'s. */
export async function acpListMemoryProposals(sessionId: string): Promise<MemoryProposalDto[]> {
  const client = await getAcpClient();
  const { proposals } = await client.goose.memoryProposalsList_unstable({ sessionId });
  return proposals;
}

/** Save writes the (edited) text as a memory through the engine's one memory writer; decline writes nothing. */
export async function acpAnswerMemoryProposal(
  sessionId: string,
  proposal: Pick<MemoryProposalDto, 'id' | 'key'>,
  decision: MemoryProposalDecision,
  text?: string
): Promise<AnswerMemoryProposalResponse_unstable> {
  const client = await getAcpClient();
  return client.goose.memoryProposalsAnswer_unstable({
    sessionId,
    key: proposal.key,
    proposalId: proposal.id,
    decision,
    ...(text != null ? { text } : {}),
  });
}
