// LeanZero brand mark — a bold, sharp-cornered "L" monogram (the logo of
// leanzero.net). Uses currentColor so it inherits the surrounding
// text/icon color, matching how the Goose mark is drawn.
export function LeanZero({ className = '' }) {
  return (
    <svg
      width="24"
      height="24"
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      <path d="M6.5 4H11V15.5H19V20H6.5V4Z" fill="currentColor" />
    </svg>
  );
}
