import { useId } from "react";

// The Navigator mark from docs/design/v2/brand: an orbit ellipse, a pilot arrow and a dot on
// a blue-to-violet squircle. Brand colours are fixed by the brand board and never recoloured.

const SQUIRCLE = "M50,0 C88,0 100,12 100,50 C100,88 88,100 50,100 C12,100 0,88 0,50 C0,12 12,0 50,0 Z";

function Glyph({ fill }: { fill: string }) {
  return (
    <>
      <ellipse cx="50" cy="52" rx="36" ry="14" fill="none" stroke={fill} strokeOpacity="0.6" strokeWidth="2.6" transform="rotate(-24 50 52)" />
      <path d="M30.6 46.2 L70.2 28.6 L52.6 68.2 L48.2 50.6 Z" fill={fill} stroke={fill} strokeWidth="3" strokeLinejoin="round" />
      <circle cx="80.5" cy="38.5" r="3.6" fill={fill} />
    </>
  );
}

/** The app icon. Below 32 px the brand rules drop its shadow. */
export function NavigatorIcon({ size = 36 }: { size?: number }) {
  const id = useId().replace(/:/g, "");
  const shadow = size >= 32;
  return (
    <svg width={size} height={Math.round(size * 1.03)} viewBox="-6 -4 112 116" aria-hidden="true" focusable="false" style={{ display: "block", overflow: "visible" }}>
      <defs>
        <linearGradient id={`bg${id}`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#6CC4FF" />
          <stop offset="0.55" stopColor="#2F6FED" />
          <stop offset="1" stopColor="#6D3FE0" />
        </linearGradient>
        <linearGradient id={`hl${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#FFFFFF" stopOpacity="0.55" />
          <stop offset="0.5" stopColor="#FFFFFF" stopOpacity="0.06" />
          <stop offset="1" stopColor="#FFFFFF" stopOpacity="0" />
        </linearGradient>
        <radialGradient id={`gl${id}`} cx="0.3" cy="0.15" r="0.7">
          <stop offset="0" stopColor="#FFFFFF" stopOpacity="0.45" />
          <stop offset="1" stopColor="#FFFFFF" stopOpacity="0" />
        </radialGradient>
        <filter id={`sh${id}`} x="-30%" y="-30%" width="160%" height="170%">
          <feDropShadow dx="0" dy="3" stdDeviation="3.5" floodColor="#2F6FED" floodOpacity="0.45" />
        </filter>
        <filter id={`gs${id}`}>
          <feDropShadow dx="0" dy="1.5" stdDeviation="1.2" floodColor="#000000" floodOpacity="0.22" />
        </filter>
      </defs>
      <path d={SQUIRCLE} fill={`url(#bg${id})`} filter={shadow ? `url(#sh${id})` : undefined} />
      <path d={SQUIRCLE} fill={`url(#gl${id})`} />
      <g filter={`url(#gs${id})`}>
        <Glyph fill="#FFFFFF" />
      </g>
      <path d="M50,1.5 C86,1.5 98.5,11 98.5,44 C80,36 20,36 1.5,44 C1.5,11 14,1.5 50,1.5 Z" fill={`url(#hl${id})`} />
      <path d={SQUIRCLE} fill="none" stroke="#FFFFFF" strokeOpacity="0.55" strokeWidth="1" />
    </svg>
  );
}

/** The one-colour menu-bar glyph, drawn in the current text colour. */
export function NavigatorGlyph({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="14 14 72 72" aria-hidden="true" focusable="false" style={{ display: "block" }}>
      <Glyph fill="currentColor" />
    </svg>
  );
}

/** "Kleari" with "Pilot" filled by the wordmark gradient. */
export function Wordmark({ className = "v2-wordmark" }: { className?: string }) {
  return (
    <span className={className}>
      Mac<span className="v2-wordmark-pilot">Pilot</span>
    </span>
  );
}
