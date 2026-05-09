// Derives the "where does this card live?" badge — local-only,
// GitHub-backed, GitLab-backed, or some other external link. Drives
// the chip rendered in the bottom-right corner of every kanban card.
//
// The signal is `external_id` + `external_url`. external_id format
// matches what the issue-scan parsers write:
//   '#NNN'  → GitHub
//   '!NNN'  → GitLab
// Anything else with a URL gets rendered as a generic external link.

import type { WorkspaceBoardCard } from './types';

export type CardSourceKind = 'local' | 'github' | 'gitlab' | 'external';

export interface CardSourceBadge {
  kind: CardSourceKind;
  label: string;
  url: string | null;
}

export function cardSourceBadge(card: WorkspaceBoardCard): CardSourceBadge {
  const id = card.externalId?.trim() ?? '';
  const url = card.externalUrl?.trim() || null;
  if (!id && !url) return { kind: 'local', label: 'local', url: null };

  // Prefer the external_id prefix as the source tell — most reliable
  // signal because both parsers stamp it. Fall back to the URL host
  // when external_id is missing (e.g. card was created by hand).
  const lowerUrl = (url ?? '').toLowerCase();
  if (id.startsWith('#') || lowerUrl.includes('github.com')) {
    return { kind: 'github', label: id || 'github', url };
  }
  if (id.startsWith('!') || lowerUrl.includes('gitlab')) {
    return { kind: 'gitlab', label: id || 'gitlab', url };
  }
  return { kind: 'external', label: id || 'link', url };
}
