<script lang="ts">
  import { mode } from '$lib/stores/app';
  import { activeHistoryEntry } from '$lib/modes/rest/stores';
  import AgentPanel from '$lib/modes/agent/components/AgentPanel.svelte';
  import RestPanel from '$lib/modes/rest/components/RestPanel.svelte';
  import SqlPanel from '$lib/modes/sql/components/SqlPanel.svelte';
  import NoSqlPanel from '$lib/modes/nosql/components/NoSqlPanel.svelte';
  import SshPanel from '$lib/modes/ssh/components/SshPanel.svelte';
  import ExplorerPanel from '$lib/modes/explorer/components/ExplorerPanel.svelte';
  import WorkspacePanel from '$lib/modes/workspace/components/WorkspacePanel.svelte';
  import HistoryViewer from '$lib/modes/rest/components/HistoryViewer.svelte';
</script>

<!--
  All mode panels are mounted continuously; only the active one is visible.
  This preserves expensive per-mode state (xterm terminals + SSH Handles in
  Agent / SSH, SFTP sessions in Explorer, CodeMirror editors + result tables
  in SQL/NoSQL, scroll/focus state everywhere) across mode switches.

  Previously this used `{#if mode === 'X'}` per panel, which unmounted the
  panel on every mode switch and triggered each panel's `onDestroy` — that
  killed terminal PTYs, SSH `Handle`s, and SFTP sessions. Switching back
  reconnected from scratch (re-auth, OTP prompt, etc.).

  Stacking with `position: absolute; inset: 0` + visibility/pointer-events
  toggle keeps each panel sized correctly even while hidden (visibility:
  hidden keeps layout). Performance cost: idle panels hold a Svelte
  component but no active resources (terminals only spawn when the user
  opens a session inside that mode).
-->
<div class="workspace">
  <div class="panel" class:active={$mode === 'agent'}>
    <AgentPanel />
  </div>

  <div class="panel" class:active={$mode === 'history'}>
    {#if $activeHistoryEntry}
      <HistoryViewer />
    {:else}
      <div class="history-empty">
        <svg viewBox="0 0 24 24" width="36" height="36"><circle cx="12" cy="12" r="10" stroke="var(--t4)" fill="none" stroke-width="1.2"/><path d="M12 6v6l4 2" stroke="var(--t4)" fill="none" stroke-width="1.2" stroke-linecap="round"/></svg>
        <span>Select an entry from history to view details</span>
      </div>
    {/if}
  </div>

  <div class="panel" class:active={$mode === 'rest'}>
    <RestPanel />
  </div>

  <div class="panel" class:active={$mode === 'sql'}>
    <SqlPanel />
  </div>

  <div class="panel" class:active={$mode === 'nosql'}>
    <NoSqlPanel />
  </div>

  <div class="panel" class:active={$mode === 'ssh'}>
    <SshPanel />
  </div>

  <div class="panel" class:active={$mode === 'explorer'}>
    <ExplorerPanel />
  </div>

  <div class="panel" class:active={$mode === 'workspace'}>
    <WorkspacePanel />
  </div>
</div>

<style>
  .workspace {
    flex: 1;
    /* Becomes the containing block for the absolutely-positioned panels. */
    position: relative;
    min-height: 0;
    overflow: hidden;
  }
  .panel {
    /* Stack — all panels share the same rectangle, fill the workspace. */
    position: absolute;
    inset: 0;
    display: flex;
    /* Hidden by default. `visibility: hidden` (rather than display:none)
       keeps each panel's layout calculated, so xterm.js/CodeMirror don't
       see a 0×0 container and miscalibrate when the panel becomes active. */
    visibility: hidden;
    pointer-events: none;
  }
  .panel.active {
    visibility: visible;
    pointer-events: auto;
    /* Float above siblings — needed because all panels share the same
       z-index plane otherwise. */
    z-index: 1;
  }
  .history-empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--t3);
    font-size: 13px;
    font-family: var(--ui);
  }
</style>
