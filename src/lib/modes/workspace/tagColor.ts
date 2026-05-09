// Deterministic tag-color picker. The same tag string always hashes to
// the same palette entry, so "bug" stays red across cards and refreshes
// without us tracking colors per-tag in the database.
//
// Palette is hand-tuned for dark theme contrast (paired text + bg) and
// keeps the saturation/lightness consistent so a board with many tags
// reads as a coherent set, not a clown show. Twelve buckets is plenty —
// collisions become noticeable around 5+ shared tags but that's fine.

export interface TagColor {
  fg: string; // text colour
  bg: string; // tinted background
  border: string; // optional border tint
}

const PALETTE: TagColor[] = [
  { fg: '#f85149', bg: 'rgba(248, 81, 73, 0.14)',  border: 'rgba(248, 81, 73, 0.35)' },   // red
  { fg: '#fb8500', bg: 'rgba(251, 133, 0, 0.14)',  border: 'rgba(251, 133, 0, 0.35)' },   // orange
  { fg: '#d29922', bg: 'rgba(210, 153, 34, 0.16)', border: 'rgba(210, 153, 34, 0.40)' },  // amber
  { fg: '#a8b730', bg: 'rgba(168, 183, 48, 0.16)', border: 'rgba(168, 183, 48, 0.40)' },  // lime
  { fg: '#3fb950', bg: 'rgba(63, 185, 80, 0.14)',  border: 'rgba(63, 185, 80, 0.35)' },   // green
  { fg: '#2ee08a', bg: 'rgba(46, 224, 138, 0.14)', border: 'rgba(46, 224, 138, 0.35)' },  // mint
  { fg: '#3ad6c0', bg: 'rgba(58, 214, 192, 0.14)', border: 'rgba(58, 214, 192, 0.35)' },  // teal
  { fg: '#58a6ff', bg: 'rgba(88, 166, 255, 0.14)', border: 'rgba(88, 166, 255, 0.35)' },  // blue
  { fg: '#7c5cf8', bg: 'rgba(124, 92, 248, 0.16)', border: 'rgba(124, 92, 248, 0.40)' },  // indigo
  { fg: '#a78bfa', bg: 'rgba(167, 139, 250, 0.16)', border: 'rgba(167, 139, 250, 0.40)' },// purple
  { fg: '#f08bb8', bg: 'rgba(240, 139, 184, 0.14)', border: 'rgba(240, 139, 184, 0.35)' },// pink
  { fg: '#9ca3af', bg: 'rgba(156, 163, 175, 0.14)', border: 'rgba(156, 163, 175, 0.35)' },// neutral
];

/** Curated overrides for tags whose colour everyone already has a
 *  mental model for. Lookups are case-insensitive and matched against
 *  the trimmed tag. Anything not in here falls through to the hash. */
const SEMANTIC: Record<string, number> = {
  // Severity / priority
  'bug': 0, 'critical': 0, 'p0': 0, 'breaking': 0, 'security': 0,
  'p1': 1, 'high': 1, 'urgent': 1,
  'p2': 2, 'wip': 2, 'in-progress': 2, 'review': 9,
  'p3': 11, 'low': 11,
  // Type
  'feature': 7, 'enhancement': 7, 'idea': 7,
  'docs': 6, 'documentation': 6,
  'spec': 8, 'design': 8, 'architecture': 9,
  'test': 4, 'tests': 4,
  // State
  'done': 5, 'complete': 5, 'shipped': 5,
  'blocked': 0, 'stuck': 0,
  // Other
  'good first issue': 4, 'help wanted': 7, 'duplicate': 11, 'invalid': 11, 'wontfix': 11,
};

function hash(s: string): number {
  let h = 5381;
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) + h) + s.charCodeAt(i);
    h |= 0;
  }
  return Math.abs(h);
}

export function tagColor(tag: string): TagColor {
  const key = tag.trim().toLowerCase();
  const semantic = SEMANTIC[key];
  const idx = semantic !== undefined ? semantic : (hash(key) % PALETTE.length);
  return PALETTE[idx];
}
