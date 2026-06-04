<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { mode } from '$lib/stores/app';
  import {
    attachSqlEditor,
    detachSqlEditor,
  } from '../services/sqlEditorReparent';
  import {
    sqlTabState,
    cancelQuery,
    ensureConnected,
    connections,
    poolStates,
    poolErrors,
    poolKey,
  } from '../stores';
  import {
    executeSqlForTab,
  } from '../services/sqlExecuteService';
  import ResultsTable from './ResultsTable.svelte';

  interface Props {
    tabId: number;
  }

  let { tabId }: Props = $props();

  let editorHost: HTMLDivElement | undefined = $state();
  let editorHeight = $state(50);
  let dragging = $state(false);
  let containerEl: HTMLDivElement | undefined = $state();
  let elapsedMs = $state(0);

  // Per-tab reactive state straight off the same store SqlPanel reads.
  // Anything the home panel writes via setSqlTabData lands here too, so
  // results stay in sync between Atlas tiles and the home view.
  const tabData = $derived($sqlTabState.get(tabId) ?? null);
  const binding = $derived(tabData?.binding ?? null);
  const inFlight = $derived(tabData?.inFlight ?? null);
  const currentResults = $derived(tabData?.results ?? []);
  const currentActiveResultIdx = $derived(tabData?.activeResultIdx ?? 0);
  const currentQuery = $derived(tabData?.query ?? '');

  const activeResultEntry = $derived(
    currentResults.length > 0 && currentActiveResultIdx < currentResults.length
      ? currentResults[currentActiveResultIdx]
      : null,
  );
  const currentResult = $derived(activeResultEntry?.result ?? tabData?.result ?? null);
  const currentError = $derived(activeResultEntry?.error ?? tabData?.error ?? null);

  const boundConnection = $derived(
    binding ? $connections.find((c) => c.id === binding.connectionId) ?? null : null,
  );
  const currentPoolKey = $derived(binding ? poolKey(binding.connectionId, binding.database) : '');
  const poolState = $derived(currentPoolKey ? $poolStates.get(currentPoolKey) ?? 'idle' : 'idle');
  const poolError = $derived(currentPoolKey ? $poolErrors.get(currentPoolKey) ?? null : null);
  const isConnecting = $derived(poolState === 'connecting');

  // Mount the singleton CodeMirror EditorView into our host div on
  // first attach. The reparent service shows the in-tile header strip
  // (connection picker) so the tile is self-sufficient — no need to
  // rely on the home action bar.
  onMount(() => {
    if (editorHost) attachSqlEditor(tabId, editorHost, { showHeader: true });
  });

  // Mirror QueryEditor's reattach effect: the singleton can only live
  // in one DOM slot. When `$mode` flips to `canvas` (Atlas) the tile is
  // visible — re-claim the editor so the home host doesn't keep it
  // when a user toggles back via the topbar.
  $effect(() => {
    if ($mode === 'canvas' && editorHost) {
      attachSqlEditor(tabId, editorHost, { showHeader: true });
    }
  });

  onDestroy(() => {
    if (editorHost) detachSqlEditor(tabId, editorHost);
  });

  // Elapsed-time counter while a query is in flight (same shape as SqlPanel).
  $effect(() => {
    if (!inFlight) {
      elapsedMs = 0;
      return;
    }
    const startedAt = inFlight.startedAt;
    elapsedMs = Date.now() - startedAt;
    const id = setInterval(() => {
      elapsedMs = Date.now() - startedAt;
    }, 100);
    return () => clearInterval(id);
  });

  async function handleRun() {
    const q = currentQuery.trim();
    if (!q) return;
    await executeSqlForTab(tabId, q);
  }

  async function handleCancel() {
    await cancelQuery(tabId);
  }

  function retryConnect() {
    if (!binding) return;
    ensureConnected(binding.connectionId, binding.database).catch(() => {
      /* surfaced via poolErrors */
    });
  }

  function setActiveResult(idx: number) {
    const entry = currentResults[idx];
    if (!entry) return;
    sqlTabState.update((m) => {
      const next = new Map(m);
      const prev = next.get(tabId);
      if (!prev) return m;
      next.set(tabId, {
        ...prev,
        activeResultIdx: idx,
        result: entry.result,
        error: entry.error,
      });
      return next;
    });
  }

  function handleDividerMousedown(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
    const startY = e.clientY;
    const startHeight = editorHeight;
    const onMousemove = (ev: MouseEvent) => {
      if (!containerEl) return;
      const rect = containerEl.getBoundingClientRect();
      const deltaPct = ((ev.clientY - startY) / rect.height) * 100;
      editorHeight = Math.max(15, Math.min(85, startHeight + deltaPct));
    };
    const onMouseup = () => {
      dragging = false;
      window.removeEventListener('mousemove', onMousemove);
      window.removeEventListener('mouseup', onMouseup);
    };
    window.addEventListener('mousemove', onMousemove);
    window.addEventListener('mouseup', onMouseup);
  }

  function fmtElapsed(ms: number): string {
    return `${(ms / 1000).toFixed(1)}s`;
  }
</script>

<div class="sql-tile-body" bind:this={containerEl}>
  <div class="sql-tile-editor" style="height:{editorHeight}%" bind:this={editorHost}></div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="sql-tile-divider" class:active={dragging} onmousedown={handleDividerMousedown}></div>

  <div class="sql-tile-actions">
    {#if inFlight}
      <span class="sql-tile-elapsed">⏱ {fmtElapsed(elapsedMs)}</span>
      <button class="sql-tile-cancel" onclick={handleCancel} title="Cancel running query">
        Cancel
      </button>
    {/if}
    <button
      class="sql-tile-run"
      onclick={handleRun}
      disabled={!!inFlight || isConnecting || !binding || !currentQuery.trim()}
      title="Run query"
    >
      {#if inFlight}
        Running…
      {:else if isConnecting}
        Connecting…
      {:else}
        Run &#9654;
      {/if}
    </button>
  </div>

  <div class="sql-tile-results" style="height:{100 - editorHeight}%">
    {#if currentResults.length > 0 && !inFlight && !isConnecting && poolState !== 'error'}
      <div class="result-tabs">
        {#each currentResults as entry, idx}
          <button
            class="result-tab"
            class:active={idx === currentActiveResultIdx}
            class:has-error={!!entry.error}
            onclick={() => setActiveResult(idx)}
            title={entry.query}
          >
            <span class="result-tab-label">{entry.label}</span>
            {#if entry.result}
              <span class="result-tab-count">{entry.result.rows.length}</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}
    <ResultsTable
      result={currentResult}
      error={currentError}
      loading={!!inFlight}
      tabId={tabId}
      query={activeResultEntry?.query ?? currentQuery}
      startedAt={activeResultEntry?.startedAt}
      liveConnectionId={binding ? `${binding.connectionId}:${binding.database}` : ''}
      databaseName={binding?.database ?? ''}
      poolState={isConnecting ? 'connecting' : poolState === 'error' ? 'error' : 'idle'}
      connectingLabel={boundConnection && binding ? `${boundConnection.name} / ${binding.database}` : ''}
      connectError={poolError}
      elapsedMs={elapsedMs}
      oncancel={handleCancel}
      onretry={retryConnect}
    />
  </div>
</div>

<style>
  .sql-tile-body {
    display: flex;
    flex-direction: column;
    flex: 1;
    height: 100%;
    width: 100%;
    overflow: hidden;
    background: var(--n);
    color: var(--t1);
    font-family: var(--ui);
    min-height: 0;
  }

  .sql-tile-editor {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 80px;
    min-width: 0;
  }

  .sql-tile-divider {
    height: 4px;
    flex-shrink: 0;
    background: var(--b1);
    cursor: row-resize;
    transition: background 0.12s;
  }
  .sql-tile-divider:hover,
  .sql-tile-divider.active {
    background: var(--acc);
  }

  .sql-tile-actions {
    height: 34px;
    flex-shrink: 0;
    background: var(--n2);
    border-top: 1px solid var(--b1);
    border-bottom: 1px solid var(--b1);
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 0 10px;
  }

  .sql-tile-elapsed {
    font-size: 11.5px;
    font-family: var(--mono);
    color: var(--t3);
    margin-right: auto;
  }

  .sql-tile-run {
    height: 24px;
    padding: 0 14px;
    border-radius: 6px;
    border: none;
    background: var(--acc);
    color: #fff;
    font-family: var(--ui);
    font-size: 11.5px;
    font-weight: 600;
    cursor: default;
    transition: opacity 0.12s;
  }
  .sql-tile-run:hover:not(:disabled) {
    opacity: 0.85;
  }
  .sql-tile-run:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .sql-tile-cancel {
    height: 24px;
    padding: 0 12px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, var(--err) 50%, transparent);
    background: transparent;
    color: var(--err);
    font-family: var(--ui);
    font-size: 11.5px;
    font-weight: 600;
    cursor: default;
    transition: background 0.12s;
  }
  .sql-tile-cancel:hover {
    background: color-mix(in srgb, var(--err) 12%, transparent);
  }

  .sql-tile-results {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 60px;
  }

  .result-tabs {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--b1);
    background: var(--n2);
    overflow-x: auto;
    flex-shrink: 0;
  }
  .result-tabs::-webkit-scrollbar { height: 2px; }
  .result-tabs::-webkit-scrollbar-thumb { background: var(--b1); border-radius: 1px; }

  .result-tab {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    border-radius: 5px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--t3);
    font-size: 11px;
    font-family: var(--ui);
    cursor: default;
    white-space: nowrap;
    transition: all 0.1s;
    max-width: 200px;
  }
  .result-tab:hover { background: var(--c); color: var(--t2); }
  .result-tab.active {
    background: color-mix(in srgb, var(--acc) 10%, transparent);
    color: var(--t1);
    border-color: color-mix(in srgb, var(--acc) 30%, transparent);
    font-weight: 500;
  }
  .result-tab.has-error { color: var(--err); }
  .result-tab-label {
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 140px;
  }
  .result-tab-count {
    font-size: 9px;
    background: var(--surface-hover);
    padding: 1px 4px;
    border-radius: 3px;
    color: var(--t3);
    flex-shrink: 0;
  }
  .result-tab.active .result-tab-count {
    background: color-mix(in srgb, var(--acc) 15%, transparent);
    color: var(--acc);
  }
</style>
