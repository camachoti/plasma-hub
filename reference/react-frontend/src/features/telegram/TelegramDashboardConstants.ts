export const QUICK_REACTIONS = ['👍', '❤️', '😂', '🔥', '🥰', '👏', '😁', '🤔', '🤯', '😱', '😢', '🎉', '🙏', '👌', '💯', '🤣', '🤩', '🤮', '💩', '🖕', '😈'];

export const TOPIC_ICON_COLORS = [
  { label: 'Vermelho', value: 16711680, css: '#ff4444' },
  { label: 'Laranja', value: 16744272, css: '#ff9010' },
  { label: 'Violeta', value: 7322096, css: '#6f48eb' },
  { label: 'Verde', value: 528304, css: '#00a152' },
  { label: 'Ciano', value: 3284671, css: '#32b3ff' },
  { label: 'Rosa', value: 14318475, css: '#da8aff' },
];

const PLASMA_COLORS = ['rose', 'violet', 'cyan', 'amber', 'emerald', 'fuchsia', 'sky'] as const;
type PlasmaColor = typeof PLASMA_COLORS[number];

export const hashColor = (str: string | undefined | null): PlasmaColor => {
  if (!str || typeof str !== 'string') return 'cyan';
  let h = 0;
  for (let i = 0; i < str.length; i++) h = ((h << 5) - h + str.charCodeAt(i)) & 0x7fffffff;
  return PLASMA_COLORS[Math.abs(h) % PLASMA_COLORS.length];
};
