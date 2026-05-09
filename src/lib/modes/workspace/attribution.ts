// Attribution helpers — produce the actor string passed to every
// workspace mutation, and render that string back to a human-friendly
// label + avatar source in the UI.
//
// Format on the wire (matches Rust side):
//   'user'                 — anonymous (no GitHub sync)
//   'user:<github-login>'  — GitHub-synced human user
//   'claude' | 'codex' | …  — agent CLI id (matches CliRunner.id())

import { get } from 'svelte/store';
import { githubConnected, githubUsername, githubAvatarUrl } from '$lib/stores/github';

/** The actor string for the currently signed-in human user. Read at
 *  the call site so a mid-session GitHub login flips attribution from
 *  'user' to 'user:<login>' immediately. */
export function currentUserActor(): string {
  if (get(githubConnected)) {
    const u = get(githubUsername);
    if (u && u.trim()) return `user:${u.trim()}`;
  }
  return 'user';
}

/** Parse a stored actor string into render data for the UI. */
export function describeActor(actor: string): {
  kind: 'user' | 'user-anon' | 'agent';
  label: string;
  agentId: string | null;
  avatarUrl: string | null;
} {
  if (actor === 'user' || !actor) {
    return { kind: 'user-anon', label: 'You', agentId: null, avatarUrl: null };
  }
  if (actor.startsWith('user:')) {
    const login = actor.slice(5).trim();
    const me = get(githubUsername);
    const avatar = me === login ? get(githubAvatarUrl) : null;
    return { kind: 'user', label: login || 'You', agentId: null, avatarUrl: avatar };
  }
  // Anything else is the agent's CLI id (claude / codex / gemini / …).
  return {
    kind: 'agent',
    label: actor.charAt(0).toUpperCase() + actor.slice(1),
    agentId: actor,
    avatarUrl: null,
  };
}

/** Format an attribution line as a single short string, e.g.
 *  "ansxuman · 2m ago" or "Claude · just now". */
export function formatAttribution(actor: string, isoTimestamp: string): string {
  const { label } = describeActor(actor);
  const when = relativeTime(isoTimestamp);
  return when ? `${label} · ${when}` : label;
}

function relativeTime(iso: string): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (isNaN(d.getTime())) return '';
  const diff = Date.now() - d.getTime();
  if (diff < 0) return 'just now';
  if (diff < 60_000) return 'just now';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  if (diff < 604_800_000) return `${Math.floor(diff / 86_400_000)}d ago`;
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}
