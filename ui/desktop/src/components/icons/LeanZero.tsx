import { LEANZERO_MARK, LEANZERO_MARK_VIEWBOX } from './leanzeroMark';

// The LeanZero brand mark — the "L" monogram of leanzero.net cradling three geese in flight
// (the Goose Flock). Geometry and the reasoning behind it live in ./leanzeroMark.ts.
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
      {LEANZERO_MARK.map((p, i) => (
        <path key={i} d={p.d} transform={p.transform} />
      ))}
    </svg>
  );
}
