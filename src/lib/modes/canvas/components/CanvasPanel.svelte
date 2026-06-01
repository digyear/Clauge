<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    loadCanvas,
    flushViewportNow,
    flushDirtyTilesNow,
    setActiveWorkspace,
    viewport,
  } from '$lib/modes/canvas/stores/canvasStore';
  import { canvasGetViewport } from '$lib/modes/canvas/commands';
  import { canvasAdapterRegistry } from '$lib/modes/canvas/adapter-registry';
  import { agentTerminalAdapter } from '$lib/modes/agent/canvas-adapter';
  import { sshTerminalAdapter } from '$lib/modes/ssh/canvas-adapter';
  import { shellTerminalAdapter } from '$lib/modes/canvas/adapters/shellTerminalAdapter';
  import { loadCanvasSettings } from '$lib/modes/canvas/stores/canvasSettingsStore';
  import CanvasViewport from './CanvasViewport.svelte';

  // Phase 2 stub: hardcoded workspace id so the surface mounts. Phase 4
  // wires this to the real active-workspace store.
  const ACTIVE_WORKSPACE_ID = '__phase2_stub__';

  // Clear stale registrations (e.g. HMR) before registering real adapters.
  canvasAdapterRegistry.clear();
  canvasAdapterRegistry.register(agentTerminalAdapter);
  canvasAdapterRegistry.register(sshTerminalAdapter);
  canvasAdapterRegistry.register(shellTerminalAdapter);

  let resolveTimer: ReturnType<typeof setTimeout> | null = null;
  let unsubscribes: Array<() => void> = [];

  async function resolveTilesNow() {
    if (resolveTimer) {
      clearTimeout(resolveTimer);
      resolveTimer = null;
    }
    const agentTabs = agentTerminalAdapter
      .listOpenTabs(ACTIVE_WORKSPACE_ID)
      .map((t) => ({ tabKind: 'agent_terminal' as const, tabId: t.id }));
    const sshTabs = sshTerminalAdapter
      .listOpenTabs(ACTIVE_WORKSPACE_ID)
      .map((t) => ({ tabKind: 'ssh_terminal' as const, tabId: t.id }));
    const shellTabs = shellTerminalAdapter
      .listOpenTabs(ACTIVE_WORKSPACE_ID)
      .map((t) => ({ tabKind: 'shell_terminal' as const, tabId: t.id }));
    await loadCanvas(ACTIVE_WORKSPACE_ID, [...agentTabs, ...sshTabs, ...shellTabs]);
  }

  function scheduleResolve() {
    if (resolveTimer) clearTimeout(resolveTimer);
    resolveTimer = setTimeout(() => {
      resolveTimer = null;
      void resolveTilesNow();
    }, 150);
  }

  onMount(async () => {
    setActiveWorkspace(ACTIVE_WORKSPACE_ID);
    await loadCanvasSettings();
    const v = await canvasGetViewport(ACTIVE_WORKSPACE_ID);
    viewport.set({ offsetX: v.offsetX, offsetY: v.offsetY, zoom: v.zoom });

    // Initial resolve.
    await resolveTilesNow();

    // Subscribe each adapter so newly-opened or newly-closed tabs trigger
    // a debounced re-resolve. Without this, a new shell spawn doesn't
    // appear and a closed agent session doesn't disappear until the user
    // leaves and re-enters Canvas mode.
    unsubscribes.push(
      agentTerminalAdapter.subscribe(ACTIVE_WORKSPACE_ID, scheduleResolve),
      sshTerminalAdapter.subscribe(ACTIVE_WORKSPACE_ID, scheduleResolve),
      shellTerminalAdapter.subscribe(ACTIVE_WORKSPACE_ID, scheduleResolve),
    );
  });

  onDestroy(() => {
    if (resolveTimer) {
      clearTimeout(resolveTimer);
      resolveTimer = null;
    }
    for (const u of unsubscribes) u();
    unsubscribes = [];
    // Svelte does not await async onDestroy callbacks; fire-and-forget
    // the flushes. Phase 2 has no real persistent state at risk yet.
    void flushViewportNow();
    void flushDirtyTilesNow();
  });
</script>

<div class="cv-panel">
  <CanvasViewport />
</div>

<style>
  .cv-panel {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    position: relative;
    overflow: hidden;
  }
</style>
