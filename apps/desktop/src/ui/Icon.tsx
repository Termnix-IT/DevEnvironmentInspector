import type { ReactNode } from 'react';

const paths = {
  inventory: <><rect x="3" y="3" width="6" height="6" rx="1" /><rect x="15" y="3" width="6" height="6" rx="1" /><rect x="3" y="15" width="6" height="6" rx="1" /><rect x="15" y="15" width="6" height="6" rx="1" /></>,
  diagnostics: <path d="M2 12h5l3-8 4 16 3-8h5" />,
  link: <><path d="m10 14 4-4M8 16l-2 2a4 4 0 0 1-6-6l5-5a4 4 0 0 1 6 0M16 8l2-2a4 4 0 0 1 6 6l-5 5a4 4 0 0 1-6 0" transform="translate(1 0) scale(.9)" /></>,
  settings: <><path d="m9 3 1-2h4l1 2 3 2 2 .2 2 3-1 2v3l1 2-2 3-2 .2-3 2-1 2h-4l-1-2-3-2-2-.2-2-3 1-2v-3l-1-2 2-3L6 5Z" transform="translate(1 1) scale(.9)" /><circle cx="12" cy="12" r="3" /></>,
  search: <><circle cx="10.5" cy="10.5" r="6.5" /><path d="m16 16 5 5" /></>,
  refresh: <><path d="M20 7V3l-3 3a8 8 0 0 0-13 5M4 17v4l3-3a8 8 0 0 0 13-5M20 3h-5M4 21h5" /></>,
  more: <><circle cx="12" cy="5" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="12" cy="19" r="1" /></>,
  chevron: <path d="m9 5 7 7-7 7" />,
  copy: <><rect x="8" y="8" width="13" height="13" rx="2" /><path d="M16 5V3H3v13h2" /></>,
  export: <><path d="M12 3v12m-4-4 4 4 4-4M4 16v5h16v-5" /></>,
  clock: <><circle cx="12" cy="12" r="9" /><path d="M12 6v6l4 2" /></>,
  close: <path d="m6 6 12 12M6 18 18 6" />,
  minimize: <path d="M5 12h14" />,
  maximize: <rect x="5" y="5" width="14" height="14" rx=".5" />,
  check: <path d="m5 12 4 4L19 6" />,
  terminal: <><rect x="2" y="4" width="20" height="16" rx="3" /><path d="m6 9 3 3-3 3m6 0h5" /></>,
  brand: <><path d="M8 8V4h10v4M16 16v4H6v-4M7 11l-3 3 3 3m10-10 3 3-3 3M10 10h4v4h-4Z" /></>,
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof paths;
export function Icon({ name, className = '' }: { name: IconName; className?: string }) {
  return <svg className={`icon ${className}`} width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}

// 同梱の小さなベクターマーク。外部画像・フォントの取得は不要。
const marks: Record<string, ReactNode> = {
  node: <><path fill="#497d31" d="m16 2 12 7v14l-12 7-12-7V9Z" /><path fill="#6fa94c" d="m16 2 12 7-12 7L4 9Z" /><path fill="#345c28" d="m16 16 12-7v14l-12 7Z" /><path fill="none" stroke="#8bc16b" d="m16 2 12 7v14l-12 7-12-7V9Zm0 14v14M4 9l12 7 12-7" /></>,
  python: <><path fill="#4e9cc9" d="M16 3C7 3 8 5 8 12H4v10h11v-8h10V7c0-4-4-4-9-4Z" /><path fill="#f7ce4e" d="M16 29c9 0 8-2 8-9h4V10H17v8H7v7c0 4 4 4 9 4Z" /><circle fill="#14202c" cx="12" cy="7" r="1.3" /><circle fill="#544520" cx="20" cy="25" r="1.3" /></>,
  git: <><rect fill="#f16a45" x="5" y="5" width="22" height="22" rx="2" transform="rotate(45 16 16)" /><path stroke="#281e1d" fill="none" strokeWidth="2" d="m9 7 13 13m-8-8v14" /><g fill="#281e1d"><circle cx="14" cy="12" r="2.5" /><circle cx="22" cy="20" r="2.5" /><circle cx="14" cy="25" r="2.5" /></g></>,
  docker: <><path fill="#36a8e3" d="M3 16h21c1-4 3-5 5-4 0 3-1 5-5 6-1 8-7 11-13 10-5-1-8-5-8-12Z" /><path stroke="#73c8f4" strokeWidth=".6" fill="#309bd1" d="M5 11h5v4H5Zm6 0h5v4h-5Zm6 0h5v4h-5ZM11 6h5v4h-5Zm6 0h5v4h-5Zm0-5h5v4h-5Z" /><circle fill="#122431" cx="8" cy="20" r="1" /></>,
  terraform: <g fill="#aa87ed"><path d="m3 4 11 6v12L3 16Zm13 7 11-6v12l-11 6Z" /><path fill="#8660ca" d="m16 25 11-6v12l-11-6Zm-2-1v7L3 25v-7Z" /></g>,
  claude: <g stroke="#dc8b68" strokeWidth="2" strokeLinecap="round">{Array.from({ length: 12 }, (_, i) => <path key={i} d="M16 3v8" transform={`rotate(${i * 30} 16 16)`} />)}</g>,
  gemini: <path fill="#9aa3fa" d="M16 1c1 9 6 14 15 15-9 1-14 6-15 15C15 22 10 17 1 16 10 15 15 10 16 1Z" />,
  codex: <><path fill="none" stroke="#e1e9e9" strokeWidth="2.2" d="m13 4 7 4v8l-7 4-7-4V8Zm7 4 7 4v8l-7 4-7-4M6 12l-4 3v8l7 4 7-4v-7" /><path fill="none" stroke="#e1e9e9" strokeWidth="2.2" d="m9 10 7-4 7 4v8l-7 4-7-4Z" /></>,
  npm: <><path fill="#d85f5b" d="M1 8h30v16H16v3H9v-3H1Z" /><path fill="#13191f" d="M5 12h6v8H8v-5H7v5H5Zm9 0h7v8h-4v3h-3Zm3 3v3h1v-3Zm6-3h5v8h-2v-5h-1v5h-2Z" /></>,
  pnpm: <g fill="#e9b345">{[0, 1, 2, 4, 5, 6, 7, 8].map((n) => <rect key={n} x={3 + (n % 3) * 9} y={3 + Math.floor(n / 3) * 9} width="7" height="7" rx=".5" />)}</g>,
  winget: <><path fill="#72b9ed" d="m3 6 12-2v11H3Zm14-2 12-2v13H17ZM3 17h12v11L3 26Zm14 0h12v13l-12-2Z" /></>,
  kubectl: <><path fill="#477bd5" d="m16 1 12 6 3 13-9 11H10L1 20 4 7Z" /><g stroke="#e7eefc" fill="none" strokeWidth="1.6"><circle cx="16" cy="16" r="8" /><circle cx="16" cy="16" r="2" /><path d="M16 5v9m0 4v9M5 16h9m4 0h9M8 8l6 6m4 4 6 6M8 24l6-6m4-4 6-6" /></g></>,
};
const letters: Record<string, { text: string; color: string }> = {
  java: { text: 'J', color: '#e89665' }, go: { text: 'GO', color: '#59c3d4' },
  rust: { text: 'R', color: '#d9c3b1' }, aws: { text: 'aws', color: '#f5bd65' },
  pip: { text: 'pip', color: '#8bb9d8' }, uv: { text: 'uv', color: '#ca9bfa' },
};
export function ToolMark({ id }: { id: string }) {
  const letter = letters[id];
  return <svg className={`tool-mark tool-mark-${id}`} width="28" height="28" viewBox="0 0 32 32" aria-hidden="true">
    {marks[id] ?? <text x="16" y="22" textAnchor="middle" fill={letter?.color ?? '#9caec0'} fontSize={letter?.text.length === 3 ? '15' : '20'} fontWeight="700" fontFamily="Consolas, monospace">{letter?.text ?? '>'}</text>}
  </svg>;
}
