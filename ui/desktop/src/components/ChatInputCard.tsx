import React from 'react';
import { SURFACE, cx } from './lz/tokens';

/**
 * Shared visual wrapper for the ChatInput.
 *
 * Both the Hub (empty-chat landing) and the BaseChat (active session)
 * present ChatInput as the Studio card on the canvas: lz-surface, a 1px
 * lz hairline, the card radius, no shadow. Centralizing it here keeps the
 * look in sync and gives a single place to tweak the recipe.
 */
export const ChatInputCard: React.FC<{
  className?: string;
  children: React.ReactNode;
}> = ({ className, children }) => (
  <div className={cx(SURFACE.card, 'overflow-hidden', className)}>{children}</div>
);
