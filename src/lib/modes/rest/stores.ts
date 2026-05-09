// REST mode runtime state — collections / requests, environments,
// per-request env overrides, and request history. Consolidated from
// former $lib/stores/{collections,environments,history}.ts.

import { writable, derived, get } from 'svelte/store';
import type {
  Collection,
  Request,
  RequestWithDetails,
  RequestUpdate,
  KVInput,
  HttpResponse,
  Environment,
  EnvVariable,
  HistoryEntry,
} from './types';
import * as cmd from './commands';
import { STORAGE_KEYS } from '$lib/shared/constants/storage';

// ── Collections / requests ─────────────────────────────────────────────

export const collections = writable<Collection[]>([]);
export const collectionsRefreshTrigger = writable(0);
export const activeCollectionId = writable<string | null>(null);
export const activeRequestId = writable<string | null>(null);
export const activeRequest = writable<RequestWithDetails | null>(null);

export const currentRestResponse = writable<HttpResponse | null>(null);

/** Per-request environment overrides (requestId/tabId -> envId) */
const savedOverrides = typeof localStorage !== 'undefined' ? JSON.parse(localStorage.getItem(STORAGE_KEYS.REQUEST_ENV_OVERRIDES) || '{}') : {};
export const requestEnvOverrides = writable<Record<string, string>>(savedOverrides);
// Keep old name as alias for backward compatibility during migration
export const collectionEnvOverrides = requestEnvOverrides;

export async function loadCollections() {
  try {
    const data = await cmd.listCollections();
    collections.set(data);
    collectionsRefreshTrigger.update(n => n + 1);
  } catch (err) {
    console.error('Failed to load collections:', err);
  }
}

export async function createCollection(name: string) {
  const coll = await cmd.createCollection(name);
  collections.update(c => [...c, coll]);
  return coll;
}

export async function deleteCollection(id: string) {
  await cmd.deleteCollection(id);
  collections.update(c => c.filter(x => x.id !== id));
  if (get(activeCollectionId) === id) {
    activeCollectionId.set(null);
  }
}

export async function updateCollection(id: string, name: string, envId: string | null) {
  const updated = await cmd.updateCollection(id, name, envId);
  collections.update(c => c.map(x => x.id === id ? updated : x));
}

export async function loadRequest(id: string) {
  const req = await cmd.getRequest(id);
  activeRequestId.set(id);
  activeRequest.set(req);
  currentRestResponse.set(null); // Clear stale response when switching requests
}

export function clearActiveRequest() {
  activeRequestId.set(null);
  activeRequest.set(null);
  currentRestResponse.set(null);
}

export async function createRequest(collectionId: string, name: string, method: string) {
  const req = await cmd.createRequest(collectionId, name, method);
  return req;
}

export async function deleteRequest(id: string) {
  await cmd.deleteRequest(id);
  activeRequest.update(r => r?.id === id ? null : r);
  if (get(activeRequestId) === id) {
    activeRequestId.set(null);
  }
}

export async function saveRequest(id: string, data: RequestUpdate) {
  await cmd.updateRequest(id, data);
}

export async function saveHeaders(requestId: string, headers: KVInput[]) {
  await cmd.updateRequestHeaders(requestId, headers);
}

export async function saveParams(requestId: string, params: KVInput[]) {
  await cmd.updateRequestParams(requestId, params);
}

export async function commitRequest(requestId: string, draft: { method?: string; url?: string; body?: string; bodyType?: string; authType?: string; authData?: string; preScript?: string; headers?: { key: string; value: string; enabled: number }[]; params?: { key: string; value: string; enabled: number }[] }) {
  const { headers, params, ...requestData } = draft;
  const hasRequestData = Object.keys(requestData).length > 0;
  if (hasRequestData) {
    await cmd.updateRequest(requestId, requestData);
  }
  if (headers) {
    await cmd.updateRequestHeaders(requestId, headers);
  }
  if (params) {
    await cmd.updateRequestParams(requestId, params);
  }
  // Reload so activeRequest and sidebar reflect saved state
  await loadRequest(requestId);
  await loadCollections();
}

export function setRequestEnv(requestOrTabId: string, envId: string | null) {
  requestEnvOverrides.update(map => {
    let next: Record<string, string>;
    if (envId === null) {
      const { [requestOrTabId]: _, ...rest } = map;
      next = rest;
    } else {
      next = { ...map, [requestOrTabId]: envId };
    }
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(STORAGE_KEYS.REQUEST_ENV_OVERRIDES, JSON.stringify(next));
    }
    return next;
  });
}

// Backward compatibility alias
export const setCollectionEnv = setRequestEnv;

// ── Environments ───────────────────────────────────────────────────────

export const environments = writable<Environment[]>([]);

// Persist active env selection
const savedEnvId = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEYS.ACTIVE_ENV_ID) : null;
export const activeEnvId = writable<string | null>(savedEnvId);

export async function loadEnvironments() {
  try {
    const envs = await cmd.listEnvironments();
    environments.set(envs);
    // Read current activeEnvId from localStorage (not the stale module-level snapshot)
    const current = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEYS.ACTIVE_ENV_ID) : null;
    const currentExists = current && envs.some(e => e.id === current);
    if (!currentExists && envs.length > 0) {
      const def = envs.find(e => e.isDefault === 1);
      if (def) setActiveEnv(def.id);
      else setActiveEnv(envs[0].id);
    } else if (!currentExists && envs.length === 0) {
      activeEnvId.set(null);
      if (typeof localStorage !== 'undefined') {
        localStorage.removeItem(STORAGE_KEYS.ACTIVE_ENV_ID);
      }
    }
  } catch (err) {
    console.error('Failed to load environments:', err);
  }
}

export async function createEnvironment(name: string, color: string) {
  const env = await cmd.createEnvironment(name, color);
  environments.update(e => [...e, env]);
  // Auto-activate if it's the first (and now default) environment
  if (env.isDefault === 1) {
    setActiveEnv(env.id);
  }
  return env;
}

export async function updateEnvironment(id: string, name: string, color: string) {
  const env = await cmd.updateEnvironment(id, name, color);
  environments.update(e => e.map(x => x.id === id ? env : x));
}

export async function deleteEnvironment(id: string) {
  await cmd.deleteEnvironment(id);
  environments.update(e => e.filter(x => x.id !== id));
  // Clear activeEnvId if the deleted env was the active one
  const currentActive = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEYS.ACTIVE_ENV_ID) : null;
  if (currentActive === id) {
    activeEnvId.set(null);
    if (typeof localStorage !== 'undefined') {
      localStorage.removeItem(STORAGE_KEYS.ACTIVE_ENV_ID);
    }
  }
  // Remove all per-request overrides pointing to the deleted env (revert to global)
  if (typeof localStorage !== 'undefined') {
    const overridesRaw = localStorage.getItem(STORAGE_KEYS.REQUEST_ENV_OVERRIDES);
    if (overridesRaw) {
      try {
        const overrides = JSON.parse(overridesRaw);
        const cleaned: Record<string, string> = {};
        for (const [key, val] of Object.entries(overrides)) {
          if (val !== id) cleaned[key] = val as string;
        }
        localStorage.setItem(STORAGE_KEYS.REQUEST_ENV_OVERRIDES, JSON.stringify(cleaned));
        requestEnvOverrides.set(cleaned);
      } catch {}
    }
  }
}

export async function setDefaultEnv(id: string) {
  await cmd.setDefaultEnvironment(id);
  activeEnvId.set(id);
  await loadEnvironments();
}

export async function setActiveEnv(id: string) {
  activeEnvId.set(id);
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(STORAGE_KEYS.ACTIVE_ENV_ID, id);
  }
}

export async function loadEnvVariables(envId: string): Promise<EnvVariable[]> {
  return cmd.listEnvVariables(envId);
}

export async function setEnvVariable(envId: string, key: string, value: string, isSecret: number) {
  return cmd.setEnvVariable(envId, key, value, isSecret);
}

export async function updateEnvVariable(id: string, key: string, value: string, isSecret: number) {
  return cmd.updateEnvVariable(id, key, value, isSecret);
}

export async function deleteEnvVariable(id: string) {
  return cmd.deleteEnvVariable(id);
}

export function getEffectiveEnvId(
  requestOrTabId: string | null,
  overrides: Record<string, string>,
  globalEnvId: string | null,
): string | null {
  if (requestOrTabId && overrides[requestOrTabId]) {
    return overrides[requestOrTabId];
  }
  return globalEnvId;
}

// ── History ────────────────────────────────────────────────────────────

export const history = writable<HistoryEntry[]>([]);
export const historyOpen = writable<boolean>(false);

/** History entries indexed by the SHARED tab id (number). The actual
 *  tab list lives in `$lib/shared/stores/tabs` so history tabs render in
 *  the global Topbar alongside REST/SQL/etc. — same look, same close UX,
 *  no duplicate tab strip. */
import {
  tabs as sharedTabs,
  activeTabId as sharedActiveTabId,
  addTab as sharedAddTab,
  closeTab as sharedCloseTab,
  activateTab as sharedActivateTab,
} from '$lib/shared/stores/tabs';

export const historyEntries = writable<Map<number, HistoryEntry>>(new Map());

/** Active entry = the entry whose tab is currently focused, IF that tab
 *  is a history tab. Returns null when the active tab is in another mode
 *  (REST request, SQL query, etc.) or when no tabs are open. */
export const activeHistoryEntry = derived(
  [sharedTabs, sharedActiveTabId, historyEntries],
  ([$tabs, $id, $entries]) => {
    const tab = $tabs.find(t => t.id === $id);
    if (!tab || tab.mode !== 'history') return null;
    return $entries.get(tab.id) ?? null;
  },
);

/** Find an existing history tab for `entryId`. */
function findHistoryTabFor(entryId: string): number | null {
  const t = get(sharedTabs).find(x => x.mode === 'history' && x.key === entryId);
  return t ? t.id : null;
}

function entryTabLabel(entry: HistoryEntry): string {
  const name = entry.requestName?.trim();
  if (name) return name;
  try { return new URL(entry.url).pathname || entry.url; } catch { return entry.url; }
}

/** Open an entry in a tab. If a tab for that entry already exists,
 *  switch to it instead of duplicating. */
export function openHistoryTab(entry: HistoryEntry) {
  const existing = findHistoryTabFor(entry.id);
  if (existing !== null) {
    sharedActivateTab(existing);
    return;
  }
  const tab = sharedAddTab(entryTabLabel(entry), 'history', entry.id, 'var(--acc)');
  historyEntries.update(m => {
    const next = new Map(m);
    next.set(tab.id, entry);
    return next;
  });
}

/** Internal — drop a history tab's stored entry data. The Topbar's
 *  doCloseTab calls this when the tab is closed (kept here so the data
 *  layer owns the cleanup). */
export function clearHistoryEntryForTab(tabId: number) {
  historyEntries.update(m => {
    if (!m.has(tabId)) return m;
    const next = new Map(m);
    next.delete(tabId);
    return next;
  });
}

export function closeAllHistoryTabs() {
  const ids = get(sharedTabs).filter(t => t.mode === 'history').map(t => t.id);
  for (const id of ids) sharedCloseTab(id);
  historyEntries.set(new Map());
}

/** Default chat-history retention when the user hasn't picked one yet.
 *  30 days is conservative enough to keep recent context while preventing
 *  the request log + AI chat from growing unbounded. */
export const DEFAULT_CHAT_RETENTION = '30d';

/** Map a retention value (24h/7d/30d/1y/never) to seconds.
 *  Returns null only for `'never'`; an unset/unknown value falls back to
 *  the default retention so first-run users get cleanup automatically. */
export function retentionSeconds(value: string | undefined | null): number | null {
  switch (value) {
    case 'never': return null;
    case '24h':   return 24 * 60 * 60;
    case '7d':    return 7 * 24 * 60 * 60;
    case '1y':    return 365 * 24 * 60 * 60;
    case '30d':
    default:      return 30 * 24 * 60 * 60;
  }
}

export async function loadHistory(limit: number = 50, retention?: string) {
  try {
    const seconds = retentionSeconds(retention);
    if (seconds !== null) {
      try { await cmd.purgeHistory(seconds); } catch { /* non-fatal */ }
    }
    const entries = await cmd.listHistory(limit);
    history.set(entries);
  } catch (err) {
    console.error('Failed to load history:', err);
  }
}

export async function clearHistory() {
  await cmd.clearHistory();
  history.set([]);
  closeAllHistoryTabs();
}

export async function deleteHistoryEntry(id: string) {
  await cmd.deleteHistoryEntry(id);
  history.update(h => h.filter(x => x.id !== id));
  // If a Topbar tab is open for this entry, close it too.
  const tabId = findHistoryTabFor(id);
  if (tabId !== null) {
    sharedCloseTab(tabId);
    clearHistoryEntryForTab(tabId);
  }
}
