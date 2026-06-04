<script lang="ts">
  import {
    connections,
    sqlTabState,
    setBinding,
    poolStates,
    poolErrors,
    reconnectingFlash,
    poolKey,
    connectionDatabases,
    loadDatabaseList,
    updateSqlScript,
  } from '../stores';
  import { tabs } from '$lib/shared/stores/tabs';
  import { descriptorFor } from '../dialects';

  interface Props {
    tabId: number;
  }

  let { tabId }: Props = $props();

  let dbDropdownOpen = $state(false);

  const currentTabData = $derived($sqlTabState.get(tabId) ?? null);
  const binding = $derived(currentTabData?.binding ?? null);

  const boundConnection = $derived(
    binding ? $connections.find((c) => c.id === binding.connectionId) ?? null : null,
  );
  const currentDbDriver = $derived(boundConnection?.driver ?? '');
  const currentPoolKey = $derived(binding ? poolKey(binding.connectionId, binding.database) : '');
  const poolState = $derived(currentPoolKey ? $poolStates.get(currentPoolKey) ?? 'idle' : 'idle');
  const poolError = $derived(currentPoolKey ? $poolErrors.get(currentPoolKey) ?? null : null);
  const isConnecting = $derived(poolState === 'connecting');
  const reconnecting = $derived(currentPoolKey ? $reconnectingFlash.has(currentPoolKey) : false);

  const sqlTab = $derived($tabs.find((t) => t.id === tabId && t.mode === 'sql') ?? null);
  const currentQuery = $derived(currentTabData?.query ?? '');

  interface DbGroup {
    connId: string;
    connName: string;
    driver: string;
    state: 'idle' | 'connecting' | 'connected' | 'error';
    databases: string[];
  }

  const dbGroups = $derived.by<DbGroup[]>(() => {
    const all = $connections;
    return all.map((conn) => {
      const cached = $connectionDatabases.get(conn.id);
      const databases = cached && cached.length > 0 ? cached : [conn.databaseName];
      const prefix = `${conn.id}:`;
      let state: DbGroup['state'] = 'idle';
      for (const [k, s] of $poolStates) {
        if (k.startsWith(prefix)) {
          if (s === 'connecting') state = 'connecting';
          else if (s === 'connected' && state !== 'connecting') state = 'connected';
          else if (s === 'error' && state === 'idle') state = 'error';
        }
      }
      return {
        connId: conn.id,
        connName: conn.name,
        driver: conn.driver,
        state,
        databases,
      };
    });
  });

  function driverLabel(driver: string): string {
    return descriptorFor(driver)?.abbreviation ?? (driver ? driver.substring(0, 2).toUpperCase() : '?');
  }

  function toggleDbDropdown(e: MouseEvent) {
    e.stopPropagation();
    dbDropdownOpen = !dbDropdownOpen;
    if (dbDropdownOpen) {
      for (const g of dbGroups) {
        if (g.state === 'connected' && !$connectionDatabases.has(g.connId)) {
          loadDatabaseList(g.connId);
        }
      }
    }
  }

  function closeDbDropdown() {
    dbDropdownOpen = false;
  }

  async function pickBinding(connId: string, db: string) {
    dbDropdownOpen = false;
    if (binding?.connectionId === connId && binding?.database === db) return;
    setBinding(tabId, connId, db);
    if (sqlTab?.key) {
      try {
        await updateSqlScript(sqlTab.key, sqlTab.label, currentQuery, db, connId);
      } catch {
        /* silent */
      }
    }
  }
</script>

<svelte:window onclick={closeDbDropdown} />

<div class="sql-tile-header">
  <div class="db-selector-wrap">
    <button
      class="db-pill"
      class:state-connecting={isConnecting}
      class:state-error={poolState === 'error'}
      onclick={toggleDbDropdown}
      title="Select target connection / database"
    >
      {#if currentDbDriver}
        <span class="db-pill-driver">{driverLabel(currentDbDriver)}</span>
      {:else}
        <svg class="db-pill-icon" viewBox="0 0 24 24">
          <ellipse cx="12" cy="5" rx="9" ry="3" />
          <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3" />
          <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5" />
        </svg>
      {/if}
      <span class="db-pill-name">
        {#if binding && boundConnection}
          {boundConnection.name} / {binding.database}
        {:else}
          Pick connection
        {/if}
      </span>
      {#if isConnecting}
        <span class="state-dot connecting" title="Connecting…"></span>
      {:else if reconnecting}
        <span class="state-dot reconnecting" title="Reconnecting…"></span>
      {:else if poolState === 'error'}
        <span class="state-dot err" title={poolError ?? 'Connection error'}></span>
      {/if}
      <svg class="db-pill-chevron" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6" /></svg>
    </button>

    {#if dbDropdownOpen}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="db-dropdown" onclick={(e) => e.stopPropagation()}>
        {#if dbGroups.length === 0}
          <div class="db-dropdown-empty">No saved connections — create one from the SQL sidebar</div>
        {:else}
          {#each dbGroups as group, gi}
            {#if gi > 0}
              <div class="db-dropdown-sep"></div>
            {/if}
            <div class="db-dropdown-group-header">
              <span class="db-group-driver">{driverLabel(group.driver)}</span>
              <span class="db-group-name">{group.connName}</span>
              {#if group.state === 'connected'}
                <span class="group-state ok" title="Pool open"></span>
              {:else if group.state === 'connecting'}
                <span class="group-state connecting" title="Connecting…"></span>
              {:else if group.state === 'error'}
                <span class="group-state err" title="Connection error"></span>
              {/if}
            </div>
            {#each group.databases as db}
              {@const sel = binding?.connectionId === group.connId && binding?.database === db}
              <button
                class="db-dropdown-item"
                class:active={sel}
                onclick={() => pickBinding(group.connId, db)}
              >
                <span class="db-dropdown-name">{db}</span>
                {#if sel}
                  <svg class="db-dropdown-check" viewBox="0 0 24 24"><path d="M20 6L9 17l-5-5" /></svg>
                {/if}
              </button>
            {/each}
          {/each}
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .sql-tile-header {
    height: 32px;
    flex-shrink: 0;
    background: var(--n2);
    border-bottom: 1px solid var(--b1);
    display: flex;
    align-items: center;
    padding: 0 8px;
    gap: 8px;
  }

  .db-selector-wrap { position: relative; }
  .db-pill {
    height: 24px;
    padding: 0 8px 0 6px;
    border-radius: 5px;
    border: 1px solid var(--b1);
    background: transparent;
    color: var(--t2);
    font-size: 12px;
    font-family: var(--mono);
    cursor: default;
    display: flex;
    align-items: center;
    gap: 5px;
    transition: border-color 0.12s, background 0.12s;
  }
  .db-pill:hover { border-color: var(--b2); background: var(--surface-hover); }
  .db-pill.state-connecting { border-color: color-mix(in srgb, var(--acc) 50%, transparent); }
  .db-pill.state-error { border-color: color-mix(in srgb, var(--err) 60%, transparent); }
  .db-pill-icon { width: 12px; height: 12px; stroke: var(--acc); fill: none; stroke-width: 1.5; stroke-linecap: round; flex-shrink: 0; opacity: 0.7; }
  .db-pill-name { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .db-pill-chevron { width: 10px; height: 10px; stroke: var(--t3); fill: none; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; flex-shrink: 0; }

  .db-pill-driver {
    font-size: 9px; font-weight: 700; color: var(--acc);
    background: color-mix(in srgb, var(--acc) 12%, transparent);
    padding: 1px 4px; border-radius: 3px; font-family: var(--mono);
    flex-shrink: 0; letter-spacing: 0.04em;
  }

  .state-dot {
    width: 7px; height: 7px; border-radius: 50%;
    flex-shrink: 0; display: inline-block;
  }
  .state-dot.connecting { background: var(--acc); animation: pulse 1.1s ease-in-out infinite; }
  .state-dot.reconnecting { background: #d97706; animation: pulse 0.8s ease-in-out infinite; }
  .state-dot.err { background: var(--err); }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.35; }
  }

  .db-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    min-width: 220px;
    max-height: 320px;
    overflow-y: auto;
    background: var(--n);
    border: 1px solid var(--b1);
    border-radius: 6px;
    padding: 4px 0;
    z-index: 100;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    animation: dbDropIn 0.1s ease;
  }
  @keyframes dbDropIn {
    from { opacity: 0; transform: translateY(-4px); }
    to { opacity: 1; transform: none; }
  }
  .db-dropdown::-webkit-scrollbar { width: 4px; }
  .db-dropdown::-webkit-scrollbar-thumb { background: var(--b1); border-radius: 2px; }

  .db-dropdown-empty { padding: 10px 14px; font-size: 11px; color: var(--t4); font-family: var(--mono); }
  .db-dropdown-group-header {
    font-size: 11px; font-weight: 600; color: var(--t3);
    text-transform: uppercase; padding: 6px 10px; font-family: var(--mono);
    letter-spacing: 0.04em; display: flex; align-items: center; gap: 6px;
    cursor: default; user-select: none;
  }
  .db-group-driver {
    font-size: 8px; font-weight: 700; color: var(--acc);
    background: color-mix(in srgb, var(--acc) 12%, transparent);
    padding: 1px 3px; border-radius: 2px; flex-shrink: 0;
  }
  .db-group-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .group-state {
    width: 6px; height: 6px; border-radius: 50%; margin-left: auto; flex-shrink: 0;
  }
  .group-state.ok { background: #10b981; }
  .group-state.connecting { background: var(--acc); animation: pulse 1.1s ease-in-out infinite; }
  .group-state.err { background: var(--err); }

  .db-dropdown-sep { height: 1px; background: var(--b1); margin: 4px 0; }
  .db-dropdown-item {
    width: 100%; padding: 5px 10px 5px 20px; border: none;
    background: transparent; color: var(--t2); font-size: 11.5px;
    font-family: var(--mono); cursor: default; text-align: left;
    display: flex; align-items: center; justify-content: space-between;
    gap: 8px; transition: background 0.08s, color 0.08s;
  }
  .db-dropdown-item:hover { background: var(--c); color: var(--t1); }
  .db-dropdown-item.active { color: var(--acc); }
  .db-dropdown-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .db-dropdown-check { width: 12px; height: 12px; stroke: var(--acc); fill: none; stroke-width: 2.5; stroke-linecap: round; stroke-linejoin: round; flex-shrink: 0; }
</style>
