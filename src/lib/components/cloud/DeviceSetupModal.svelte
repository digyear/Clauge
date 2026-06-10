<script lang="ts">
  // Set up this device — shown when BOTH this device and the cloud account
  // contain data and the device has never synced. The user picks how to
  // combine them; a snapshot is taken Rust-side before anything destructive.
  //
  //   merge → cloudMergeAll      (keeps both, newer edits win)
  //   cloud → cloudSyncRestore   (cloud copy replaces this device)
  //   keep  → cloudForcePushAll  (this device overwrites the cloud)
  //
  // All three mark has-synced server-side; markSynced() updates the
  // in-memory store. Closing without choosing is allowed — hasSyncedOnce
  // stays false so the modal reappears on the next boot.
  import { showDeviceSetup, cloudUser, markSynced } from '$lib/stores/cloud';
  import { cloudMergeAll, cloudForcePushAll, cloudSyncRestore } from '$lib/commands/cloud';
  import { reloadSyncedStores } from '$lib/commands/syncReload';
  import { showToast } from '$lib/shared/primitives/toast';

  /** Teleport the modal subtree to <body>. Same pattern as
   *  ConflictResolverModal / Modal.svelte — sidesteps clipping by
   *  transformed or overflow:hidden ancestors. */
  function teleportToBody(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        if (node.parentElement === document.body) node.remove();
      },
    };
  }

  interface Props {
    show: boolean;
  }

  let { show = $bindable() }: Props = $props();

  let busy = $state<'merge' | 'keep' | 'cloud' | null>(null);

  async function run(choice: 'merge' | 'keep' | 'cloud') {
    if (busy) return;
    busy = choice;
    try {
      if (choice === 'merge') await cloudMergeAll();
      else if (choice === 'keep') await cloudForcePushAll();
      else await cloudSyncRestore();
      markSynced();
      await reloadSyncedStores();
      showToast('Device set up', 'success');
      showDeviceSetup.set(false);
    } catch (e: any) {
      showToast(`Setup failed: ${e?.message ?? e}`, 'error');
    } finally {
      busy = null;
    }
  }

  function close() {
    if (busy) return;
    show = false;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && show) {
      e.preventDefault();
      close();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if show}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="ds-overlay" use:teleportToBody onclick={close}>
    <div class="ds-modal modal-card" onclick={(e: MouseEvent) => e.stopPropagation()} role="dialog" aria-modal="true">
      <header class="ds-hdr">
        <div class="ds-hdr-text">
          <span class="ds-title">Set up this device</span>
          {#if $cloudUser}
            <span class="ds-account">
              Signed in as {$cloudUser.email || $cloudUser.displayName || $cloudUser.slug}
            </span>
          {/if}
        </div>
        <button class="ds-close" onclick={close} aria-label="Close" disabled={!!busy}>&times;</button>
      </header>

      <div class="ds-body">
        <p class="ds-lead">
          Both this device and your cloud account contain data. Choose how
          to combine them — a snapshot of this device is saved before any
          change.
        </p>

        <div class="ds-options">
          <button class="ds-option ds-option-primary" onclick={() => run('merge')} disabled={!!busy}>
            <span class="ds-option-title">
              {busy === 'merge' ? 'Merging…' : 'Merge (recommended)'}
            </span>
            <span class="ds-option-caption">Keeps everything from both devices. Newer edits win.</span>
          </button>
          <button class="ds-option" onclick={() => run('cloud')} disabled={!!busy}>
            <span class="ds-option-title">
              {busy === 'cloud' ? 'Restoring…' : 'Use cloud copy'}
            </span>
            <span class="ds-option-caption">Replace this device's data with the cloud copy.</span>
          </button>
          <button class="ds-option" onclick={() => run('keep')} disabled={!!busy}>
            <span class="ds-option-title">
              {busy === 'keep' ? 'Pushing…' : "Keep this device's data"}
            </span>
            <span class="ds-option-caption">Overwrite the cloud with this device's data.</span>
          </button>
        </div>
      </div>

      <footer class="ds-foot">
        <button class="ds-later" onclick={close} disabled={!!busy}>
          Decide later
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .ds-overlay {
    position: fixed;
    inset: 0;
    background: var(--scrim-strong);
    z-index: var(--z-modal);
    display: flex;
    align-items: center;
    justify-content: center;
    animation: ds-fade 0.15s ease;
  }
  @keyframes ds-fade {
    from { opacity: 0; }
    to   { opacity: 1; }
  }
  .ds-modal {
    width: min(520px, 92vw);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: ds-rise 0.18s ease;
  }
  @keyframes ds-rise {
    from { opacity: 0; transform: translateY(8px) scale(0.98); }
    to   { opacity: 1; transform: none; }
  }
  .ds-hdr {
    display: flex;
    align-items: center;
    padding: 14px 18px;
    border-bottom: 1px solid var(--b1);
    background: var(--e);
  }
  .ds-hdr-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ds-title {
    font-size: 14.5px;
    font-weight: 600;
    color: var(--t1);
    font-family: var(--ui);
  }
  .ds-account {
    font-size: 11.5px;
    color: var(--t3);
    font-family: var(--ui);
  }
  .ds-close {
    margin-left: auto;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    border: 1px solid var(--b1);
    background: transparent;
    color: var(--t3);
    font-size: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: default;
    transition: background 0.12s, color 0.12s;
  }
  .ds-close:hover { background: var(--c); color: var(--t1); }
  .ds-close:disabled { opacity: 0.4; }

  .ds-body {
    padding: 18px 22px;
    color: var(--t2);
    font-family: var(--ui);
    font-size: 13px;
    line-height: 1.55;
  }
  .ds-lead { margin: 0 0 14px; }

  .ds-options {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .ds-option {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    text-align: left;
    padding: 10px 14px;
    border-radius: 8px;
    border: 1px solid var(--b1);
    background: transparent;
    font-family: var(--ui);
    cursor: default;
    transition: background 0.12s, border-color 0.12s;
  }
  .ds-option:disabled { opacity: 0.5; }
  .ds-option:hover:not(:disabled) {
    background: var(--surface-hover);
    border-color: var(--b2);
  }
  .ds-option-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--t1);
  }
  .ds-option-caption {
    font-size: 12px;
    color: var(--t3);
  }
  .ds-option-primary {
    background: var(--acc);
    border-color: var(--acc);
  }
  .ds-option-primary .ds-option-title { color: #fff; }
  .ds-option-primary .ds-option-caption { color: rgba(255, 255, 255, 0.75); }
  .ds-option-primary:hover:not(:disabled) {
    background: var(--acc);
    border-color: var(--acc);
    filter: brightness(1.08);
  }

  .ds-foot {
    display: flex;
    justify-content: flex-end;
    padding: 12px 18px 16px;
    border-top: 1px solid var(--b1);
  }
  .ds-later {
    height: 28px;
    padding: 0 10px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--t3);
    font-family: var(--ui);
    font-size: 12px;
    cursor: default;
    transition: color 0.12s, background 0.12s;
  }
  .ds-later:hover:not(:disabled) { color: var(--t1); background: var(--surface-hover); }
  .ds-later:disabled { opacity: 0.5; }
</style>
