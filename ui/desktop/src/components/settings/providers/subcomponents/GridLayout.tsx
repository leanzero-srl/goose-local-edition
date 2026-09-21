import React, { memo } from 'react';

/**
 * Two columns of content-sized cards (one column under 720px). The previous 200px tiles in an
 * auto-fill grid had a fixed 160px height, so any description longer than three lines scrolled
 * inside its card (owner, 2026-09-21: "ugly scroll bars — I want these modified to 2 columns and
 * cards that expand"). Cards align to the top of their row so a long description never stretches
 * its neighbour.
 */
export const GRID_LAYOUT_CLASS =
  'grid grid-cols-1 min-[720px]:grid-cols-2 items-start gap-4 [&_*]:z-20 p-1';

const GridLayout = memo(function GridLayout({ children }: { children: React.ReactNode }) {
  return (
    <div data-testid="provider-grid-layout" className={GRID_LAYOUT_CLASS}>
      {children}
    </div>
  );
});

export default GridLayout;
