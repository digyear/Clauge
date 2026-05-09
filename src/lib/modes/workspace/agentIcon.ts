// One source of truth for "what does provider X look like?"  Used by:
//   • CardThread chat bubbles — round avatar to the left/right of the body
//   • BoardView in-flight indicator — small pulsing chip on the card
//   • CardEditorDrawer linked-session card — leading icon
//
// Returning an SVG string lets callers drop it inline with `{@html}` —
// avoids a per-provider Svelte component file when each is just one tag.

export interface AgentIcon {
  /** Inline SVG markup. */
  svg: string;
  /** Brand-ish colour for backgrounds/borders. */
  color: string;
  /** Display label, capitalised. */
  label: string;
}

const STAR =
  '<svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l1.6 4.8L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.2L12 3z"/></svg>';

const SPARK_DOTS =
  '<svg viewBox="0 0 24 24" width="11" height="11" fill="currentColor"><circle cx="6" cy="12" r="2"/><circle cx="12" cy="12" r="2.5"/><circle cx="18" cy="12" r="2"/></svg>';

const DIAMOND =
  '<svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 3l9 9-9 9-9-9 9-9z"/></svg>';

const TERMINAL =
  '<svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>';

const ROBOT =
  '<svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="7" width="16" height="12" rx="2"/><path d="M12 7V3"/><circle cx="9" cy="13" r="1"/><circle cx="15" cy="13" r="1"/></svg>';

export function agentIcon(provider: string | null | undefined): AgentIcon {
  switch ((provider ?? '').toLowerCase()) {
    case 'claude':
      return { svg: STAR, color: '#d4a96a', label: 'Claude' };
    case 'codex':
      return { svg: TERMINAL, color: '#10a37f', label: 'Codex' };
    case 'gemini':
      return { svg: DIAMOND, color: '#4285f4', label: 'Gemini' };
    case 'opencode':
      return { svg: SPARK_DOTS, color: '#a78bfa', label: 'OpenCode' };
    case 'aider':
      return { svg: TERMINAL, color: '#f59e0b', label: 'Aider' };
    default:
      return { svg: ROBOT, color: '#9ca3af', label: provider || 'agent' };
  }
}
