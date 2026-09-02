import { LEANZERO_MARK_VIEWBOX, LeanZeroMarkContent } from './leanzeroMark';

// The LeanZero brand mark — the "L" monogram of leanzero.net with two of the ORIGINAL goose (the
// upstream product's own mark) flying out of it. Geometry and the reasoning: ./leanzeroMark.ts.
// Uses currentColor so it inherits the surrounding text/icon color, matching the Goose mark.
export function LeanZero({ className = '' }) {
  return (
    <svg
      width="24"
      height="24"
      viewBox={LEANZERO_MARK_VIEWBOX}
      fill="currentColor"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      <LeanZeroMarkContent />
    </svg>
  );
}
