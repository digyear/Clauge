import { mount, unmount } from 'svelte';
import { get } from 'svelte/store';
import type { CanvasTabAdapter } from '$lib/modes/canvas/adapter';
import { tabs, activateTab, draftRequests } from '$lib/shared/stores/tabs';
import { activeRequest, activeRequestId, loadRequest } from '$lib/modes/rest/stores';
import { setMode } from '$lib/stores/app';
import RestRequestTileBody from './components/RestRequestTileFullBody.svelte';

export const restRequestAdapter: CanvasTabAdapter = {
  tabKind: 'rest_request',
  mountStrategy: 'remount',
  defaultSpawnSize: { width: 720, height: 540 },

  listOpenTabs(_workspaceId) {
    return get(tabs)
      .filter((t) => t.mode === 'rest' && t.key)
      .map((t) => ({ id: t.key as string, title: t.label }));
  },

  subscribe(_workspaceId, onChange) {
    const unsubTabs = tabs.subscribe(() => onChange());
    const unsubDrafts = draftRequests.subscribe(() => onChange());
    return () => {
      unsubTabs();
      unsubDrafts();
    };
  },

  render(tabId, slot, _ctx) {
    const instance = mount(RestRequestTileBody, {
      target: slot,
      props: { tabId },
    });
    return {
      destroy() {
        try {
          unmount(instance);
        } catch {
          // Instance may already be torn down by HMR.
        }
      },
    };
  },

  getMeta(tabId) {
    const topbarTab = get(tabs).find((t) => t.mode === 'rest' && t.key === tabId);
    const req = get(activeRequest);
    const liveName = req && req.id === tabId ? req.name : null;
    const title = liveName ?? topbarTab?.label ?? 'Untitled';
    const dirty = topbarTab ? get(draftRequests).has(topbarTab.id) : false;
    return { title, dirty };
  },

  openInHomeMode(tabId) {
    void (async () => {
      await setMode('rest');
      const match = get(tabs).find((t) => t.mode === 'rest' && t.key === tabId);
      if (match) activateTab(match.id);
      if (get(activeRequestId) !== tabId) {
        try {
          await loadRequest(tabId);
        } catch {
          // Request may have been deleted; home mode handles the empty state.
        }
      }
    })();
  },

  closeTab(tabId) {
    const topbarTab = get(tabs).find((t) => t.mode === 'rest' && t.key === tabId);
    if (!topbarTab) return;
    window.dispatchEvent(
      new CustomEvent('canvas:request-tab-close', { detail: { tabId: topbarTab.id } }),
    );
  },
};
