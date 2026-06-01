import { get } from 'svelte/store';
import { agentTerminalMap } from '$lib/modes/agent/stores';

/**
 * Move the xterm container for `sessionId` into `slot`. No-op if there is
 * no entry yet (the session hasn't been activated in AgentPanel and so the
 * xterm hasn't been created).
 *
 * Uses native `appendChild` semantics: if the container is already parented
 * elsewhere, the browser silently removes it from the old parent first.
 * After moving, the FitAddon refits to the new container's dimensions.
 */
export function attachAgentTerminal(sessionId: string, slot: HTMLElement): void {
  const entry = get(agentTerminalMap).get(sessionId);
  if (!entry) return;
  slot.appendChild(entry.container);
  try {
    entry.fitAddon.fit();
  } catch {
    // Slot may not have measurable dimensions yet; the first ResizeObserver
    // tick after layout will fit. Swallow silently.
  }
}

/**
 * Move the xterm container OUT of `slot` if it currently sits there.
 * Safe to call when the container is already elsewhere. Does NOT destroy
 * the xterm or detach the PTY — the entry stays alive in agentTerminalMap.
 */
export function detachAgentTerminal(sessionId: string, slot: HTMLElement): void {
  const entry = get(agentTerminalMap).get(sessionId);
  if (!entry) return;
  if (entry.container.parentElement === slot) {
    slot.removeChild(entry.container);
  }
}

/**
 * Return the list of currently-open agent sessions (entries whose PTY has
 * been spawned, i.e. terminalId is non-null).
 * Used by the Canvas adapter's `listOpenTabs`.
 */
export function listOpenAgentTerminals(): { id: string; title: string }[] {
  const m = get(agentTerminalMap);
  const out: { id: string; title: string }[] = [];
  for (const [sessionId, entry] of m) {
    // Include any entry with a created xterm container, even if the PTY
    // hasn't connected yet (terminalId == null). The xterm will mount;
    // once the PTY connects, output flows into it.
    if (entry?.container) {
      out.push({ id: sessionId, title: sessionId });
    }
  }
  return out;
}
