import { get } from 'svelte/store';
import {
  sqlTabState,
  getSqlTabData,
  setSqlTabData,
  ensureConnected,
  sqlRowLimit,
  connections,
  databaseTables,
  loadTablesForDb,
} from '../stores';
import { sqlExecuteQuery, sqlExecuteBatch } from '../commands';
import type { Binding, SqlQueryResult, SqlResultEntry } from '../types';
import { translateMetaCommand } from '../utils/psqlMeta';
import { showToast } from '$lib/shared/primitives/toast';
import { friendlyError } from '$lib/utils/errors';

/**
 * Per-tab execute helpers shared by SqlPanel (home mode) and the Atlas
 * SQL tile body. Both call sites need exact same semantics: snapshot
 * binding before any await so a mid-flight tab switch can't redirect
 * results, surface the same toasts, and patch the same `sqlTabState`
 * keys (`inFlight` / `result` / `results` / `activeResultIdx` /
 * `error`). Lifting the bodies out of SqlPanel keeps both surfaces in
 * lockstep.
 */

function makeQueryId(): string {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now()}-${Math.random().toString(36).slice(2)}`
  );
}

function makeResultLabel(query: string): string {
  const trimmed = query.trim().replace(/\s+/g, ' ');
  const match = trimmed.match(
    /\b(?:FROM|INTO|UPDATE|TABLE|INDEX\s+(?:\w+\s+)?ON)\s+[`"']?(\w+)/i,
  );
  if (match) return match[1];
  return trimmed.length > 30 ? trimmed.slice(0, 30) + '...' : trimmed;
}

function applyRowLimit(query: string): string {
  const limit = get(sqlRowLimit);
  if (limit <= 0) return query;
  let trimmed = query.trim().replace(/;+\s*$/, '');
  if (!/^\s*select\b/i.test(trimmed)) return query;
  if (/\bLIMIT\s+\d+/i.test(trimmed)) return query;
  if (/\bFORMAT\s+\w+\s*$/i.test(trimmed)) return query;
  if (/\bSETTINGS\b/i.test(trimmed)) return query;
  trimmed = trimmed.replace(/--[^\n]*$/, '').trimEnd();
  return `${trimmed} LIMIT ${limit}`;
}

function rewriteMetaCommand(query: string, driver: string | undefined): string {
  if (!driver) return query;
  const rewritten = translateMetaCommand(query, driver);
  return rewritten ?? query;
}

async function refreshSchemaIfDdl(
  target: Binding,
  items: (SqlQueryResult | null | undefined)[],
): Promise<void> {
  const hasDdl = items.some((r) => r && r.queryKind === 'ddl');
  if (!hasDdl) return;
  const cacheKey = `${target.connectionId}:${target.database}`;
  databaseTables.update((m) => {
    const n = new Map(m);
    n.delete(cacheKey);
    return n;
  });
  await loadTablesForDb(target.connectionId, target.database);
}

function driverFor(connId: string): string | undefined {
  return get(connections).find((c) => c.id === connId)?.driver;
}

/**
 * Run a single statement against the binding currently set on `tabId`.
 * Mirrors SqlPanel.handleExecute exactly — same toasts, same tab-state
 * patches, same snapshot-before-await contract.
 */
export async function executeSqlForTab(tabId: number, query: string): Promise<void> {
  const data = getSqlTabData(tabId);
  const binding = data.binding;
  if (!binding) {
    showToast('Pick a connection first', 'error');
    return;
  }
  if (data.inFlight) return;

  // Snapshot binding before any await — see SqlPanel docs.
  const execBinding: Binding = binding;

  try {
    await ensureConnected(execBinding.connectionId, execBinding.database);
  } catch (e: any) {
    showToast(`Couldn't connect: ${friendlyError(e)}`, 'error');
    return;
  }

  const queryId = makeQueryId();
  const label = makeResultLabel(query);
  const driver = driverFor(execBinding.connectionId);
  const existing = getSqlTabData(tabId).results || [];
  const existingIdx = existing.findIndex((e) => e.label === label);

  const startedAt = Date.now();
  setSqlTabData(tabId, {
    inFlight: { queryId, startedAt },
    error: null,
  });

  try {
    const result = await sqlExecuteQuery(
      execBinding.connectionId,
      execBinding.database,
      applyRowLimit(rewriteMetaCommand(query, driver)),
      queryId,
    );
    const entry: SqlResultEntry = {
      label,
      query,
      result,
      error: null,
      startedAt,
    };
    let updated: SqlResultEntry[];
    let focusIdx: number;
    if (existingIdx >= 0) {
      updated = [...existing];
      updated[existingIdx] = entry;
      focusIdx = existingIdx;
    } else {
      updated = [...existing, entry];
      focusIdx = updated.length - 1;
    }
    setSqlTabData(tabId, {
      result,
      results: updated,
      activeResultIdx: focusIdx,
      inFlight: null,
    });
    showToast(`Query completed in ${result.durationMs}ms`, 'success');
    void refreshSchemaIfDdl(execBinding, [result]);
  } catch (e: any) {
    const msg = e?.toString?.() ?? String(e);
    const entry: SqlResultEntry = {
      label,
      query,
      result: null,
      error: msg,
      startedAt,
    };
    let updated: SqlResultEntry[];
    let focusIdx: number;
    if (existingIdx >= 0) {
      updated = [...existing];
      updated[existingIdx] = entry;
      focusIdx = existingIdx;
    } else {
      updated = [...existing, entry];
      focusIdx = updated.length - 1;
    }
    setSqlTabData(tabId, {
      error: msg,
      results: updated,
      activeResultIdx: focusIdx,
      inFlight: null,
    });
    showToast(friendlyError(e), 'error');
  }
}

/**
 * Run a batch of statements via `sqlExecuteBatch`. Mirrors
 * SqlPanel.handleExecuteMulti — transactional engines roll back on
 * failure, sequential engines surface partial-success messaging.
 */
export async function executeSqlBatchForTab(
  tabId: number,
  queries: string[],
): Promise<void> {
  const data = getSqlTabData(tabId);
  const binding = data.binding;
  if (!binding) {
    showToast('Pick a connection first', 'error');
    return;
  }
  if (data.inFlight) return;

  const execBinding: Binding = binding;

  try {
    await ensureConnected(execBinding.connectionId, execBinding.database);
  } catch (e: any) {
    showToast(`Couldn't connect: ${friendlyError(e)}`, 'error');
    return;
  }

  const entries: SqlResultEntry[] = queries.map((q) => ({
    label: makeResultLabel(q),
    query: q,
    result: null,
    error: null,
    startedAt: Date.now(),
  }));

  const batchId = makeQueryId();
  const batchStartedAt = Date.now();
  setSqlTabData(tabId, {
    inFlight: { queryId: batchId, startedAt: batchStartedAt },
    result: null,
    error: null,
    results: entries,
    activeResultIdx: 0,
  });

  const driver = driverFor(execBinding.connectionId) ?? '';
  const isTransactional = ['postgresql', 'mysql', 'sqlite'].includes(driver);

  const prepared = queries.map((q) =>
    applyRowLimit(rewriteMetaCommand(q, driver)),
  );

  let results: SqlQueryResult[] = [];
  let batchError: string | null = null;
  try {
    results = await sqlExecuteBatch(
      execBinding.connectionId,
      execBinding.database,
      prepared,
    );
  } catch (e: any) {
    batchError = e?.toString?.() ?? String(e);
  }

  if (batchError) {
    const firstUnsuccessful = results.length;
    for (let i = 0; i < entries.length; i++) {
      if (i < results.length) {
        entries[i].result = results[i];
      } else if (i === firstUnsuccessful) {
        entries[i].error = batchError;
      }
    }
    setSqlTabData(tabId, {
      inFlight: null,
      result: entries[entries.length - 1]?.result ?? null,
      error: batchError,
      results: entries,
      activeResultIdx: firstUnsuccessful,
    });
    if (isTransactional) {
      showToast(
        `Batch rolled back — ${queries.length - firstUnsuccessful} statements failed (no changes persisted)`,
        'error',
      );
    } else {
      showToast(
        `Batch failed at statement ${firstUnsuccessful + 1} — ${firstUnsuccessful} statement(s) already persisted (engine has no rollback)`,
        'error',
      );
    }
    return;
  }

  for (let i = 0; i < entries.length; i++) {
    entries[i].result = results[i] ?? null;
  }
  setSqlTabData(tabId, {
    inFlight: null,
    result: entries[entries.length - 1]?.result ?? null,
    error: null,
    results: entries,
    activeResultIdx: entries.length - 1,
  });
  showToast(
    isTransactional
      ? `${queries.length} statements committed atomically`
      : `${queries.length} statements completed`,
    'success',
  );
  void refreshSchemaIfDdl(execBinding, results);
}

/**
 * Read-only access for components that want to gate UI on inFlight.
 * Subscribers should still read `sqlTabState` directly for reactivity;
 * this is a convenience for non-reactive call sites.
 */
export function isTabInFlight(tabId: number): boolean {
  return get(sqlTabState).get(tabId)?.inFlight != null;
}
