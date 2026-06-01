import { writable, get } from 'svelte/store';
import type { KVInput } from '$lib/types';

export type TabMode = 'rest' | 'sql' | 'nosql' | 'agent' | 'canvas' | 'ssh' | 'explorer' | 'history' | 'workspace' | 'settings';

export interface Tab {
  id: number;
  label: string;
  mode: TabMode;
  key: string | null;
  dot: string;
  dirty: boolean;
  unsaved: boolean;
}

export interface DraftRequest {
  method: string;
  url: string;
  body: string;
  bodyType: string;
  headers: KVInput[];
  params: KVInput[];
  authType: string;
  authData: string;
  preScript: string;
}

let nextId = 0;

export const tabs = writable<Tab[]>([]);
export const activeTabId = writable<number>(-1);
export const draftRequests = writable<Map<number, Partial<DraftRequest>>>(new Map());

// Per-mode memory of "the tab the user was most recently looking at in
// this mode." Used by the sidebar so flipping Agent → REST → Agent
// returns to the EXACT tab the user had focused, not just any tab of
// that mode. Updated on every activation funnel (`activateTab`,
// `addTab`, `closeTab`'s replacement pick) so callers never have to
// touch it directly.
export const lastActiveTabPerMode = writable<Map<TabMode, number>>(new Map());

function recordActivation(id: number) {
  const tab = get(tabs).find(t => t.id === id);
  if (!tab) return;
  lastActiveTabPerMode.update(m => {
    const next = new Map(m);
    next.set(tab.mode, id);
    return next;
  });
}

export function addTab(label: string, mode: TabMode, key: string | null, dot: string): Tab {
  const isNew = key === null;
  const tab: Tab = { id: nextId++, label, mode, key, dot, dirty: false, unsaved: isNew };
  tabs.update(t => [...t, tab]);
  activeTabId.set(tab.id);
  recordActivation(tab.id);
  return tab;
}

export function closeTab(id: number) {
  tabs.update(t => {
    const closingTab = t.find(x => x.id === id);
    const filtered = t.filter(x => x.id !== id);
    // Sweep the per-mode memory: drop any entry whose tab just died,
    // so the next visit to that mode falls back to creation-order
    // instead of trying to focus a tab that no longer exists.
    lastActiveTabPerMode.update(m => {
      let changed = false;
      const next = new Map(m);
      for (const [mode, tabId] of next) {
        if (tabId === id) { next.delete(mode); changed = true; }
      }
      return changed ? next : m;
    });
    if (get(activeTabId) === id && filtered.length > 0) {
      // Prefer the most recent tab of the same mode as the closing tab —
      // keeps the user in their current mental context. Fall back to the
      // most recent tab overall (cross-mode); callers that care about
      // the resulting mode should realign $mode after this returns.
      const sameMode = closingTab
        ? filtered.filter(x => x.mode === closingTab.mode)
        : [];
      const next = sameMode.length > 0
        ? sameMode[sameMode.length - 1]
        : filtered[filtered.length - 1];
      activeTabId.set(next.id);
      recordActivation(next.id);
    } else if (filtered.length === 0) {
      activeTabId.set(-1);
    }
    return filtered;
  });
  clearDraft(id);
}

export function activateTab(id: number) {
  activeTabId.set(id);
  recordActivation(id);
}

export function updateTab(id: number, updates: Partial<Tab>) {
  tabs.update(t => t.map(x => x.id === id ? { ...x, ...updates } : x));
}

/** Open (or focus) the singleton Settings tab. Settings is cross-mode:
 *  activateTabAcrossMode does NOT call `mode.set('settings')` for these,
 *  so $mode + the "+" button stay tied to whatever real mode the user is
 *  in. `subKey` selects the inner Settings tab (e.g. 'account', 'ai',
 *  'agent', 'agent:usage', 'agent:contexts', 'agent:plugins', 'workspace'). */
export function openSettingsTab(subKey: string = 'general'): Tab {
  const existing = get(tabs).find(t => t.mode === 'settings');
  if (existing) {
    if (existing.key !== subKey) updateTab(existing.id, { key: subKey });
    activeTabId.set(existing.id);
    return existing;
  }
  return addTab('Settings', 'settings', subKey, 'var(--t3)');
}

export function setDraft(tabId: number, data: Partial<DraftRequest>) {
  draftRequests.update(m => {
    const existing = m.get(tabId) ?? {};
    const updated = { ...existing, ...data };
    const next = new Map(m);
    next.set(tabId, updated);
    return next;
  });
}

export function getDraft(tabId: number): Partial<DraftRequest> | undefined {
  return get(draftRequests).get(tabId);
}

export function clearDraft(tabId: number) {
  draftRequests.update(m => {
    const next = new Map(m);
    next.delete(tabId);
    return next;
  });
}

export function markDirty(tabId: number) {
  tabs.update(t => t.map(x => x.id === tabId ? { ...x, dirty: true } : x));
}

export function markClean(tabId: number) {
  tabs.update(t => t.map(x => x.id === tabId ? { ...x, dirty: false } : x));
}
