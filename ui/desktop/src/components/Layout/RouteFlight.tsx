import { useLocation } from 'react-router-dom';

/** Navigation feedback does not delay mounting the destination or intercept input. */
export function RouteFlight() {
  const { pathname } = useLocation();
  return (
    <div key={pathname} className="route-flight" aria-hidden="true">
      <svg
        viewBox="0 0 100 40"
        width="100"
        height="40"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {[0, 1].map((n) => (
          <g key={n} transform={`translate(${n * 40 + 5} ${n * 9 + 7})`}>
            <path
              d="M3 18 Q14 10 26 17 L32 9 Q34 5 38 8 L42 10 L36 11 L33 20 Q20 25 10 22 Z"
              fill="currentColor"
              stroke="none"
            />
            <path className="flight-wing" d="M19 18 Q12 3 3 2 M19 18 Q23 7 29 5" />
          </g>
        ))}
      </svg>
    </div>
  );
}
