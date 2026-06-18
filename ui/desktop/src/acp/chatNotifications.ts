import type { GooseSessionNotification_unstable } from '@aaif/goose-sdk';
import type { SessionNotification } from '@agentclientprotocol/sdk';
import { createSessionScopedNotificationRouter } from './sessionScopedNotificationRouter';

const acpSessionRouter = createSessionScopedNotificationRouter<SessionNotification>();
const gooseSessionRouter =
  createSessionScopedNotificationRouter<GooseSessionNotification_unstable>();

export const subscribeToAcpSession = acpSessionRouter.subscribe;
export const routeAcpSessionNotification = async (
  notification: SessionNotification
): Promise<void> => {
  await acpSessionRouter.route(notification);
};

export const subscribeToAcpGooseSession = gooseSessionRouter.subscribe;
export const routeAcpGooseSessionNotification = async (
  notification: GooseSessionNotification_unstable
): Promise<void> => {
  await gooseSessionRouter.route(notification);
};
