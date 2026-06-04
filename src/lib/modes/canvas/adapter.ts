import type { TabKind } from '$lib/modes/canvas/commands';

export type MountStrategy = 'reparent' | 'remount';

export interface TileMeta {
  title: string;
  /** Lucide icon name or SVG path data — adapter chooses how to render. */
  icon?: string;
  /** Small colored dot (status/activity). */
  statusDot?: string;
  /** Visually mark the title bar as "unsaved" / "dirty". */
  dirty?: boolean;
}

export interface CanvasTabAdapter {
  tabKind: TabKind;
  mountStrategy: MountStrategy;

  /**
   * Initial spawn size when this adapter's tile is first created. Defaults
   * to the backend's 720x480 if omitted. Used by `resolveTilesNow` so the
   * tile renders comfortably for the host content (kanban needs more, etc.).
   */
  defaultSpawnSize?: { width: number; height: number };

  /** Return currently-open tabs for this workspace (for canvas_resolve_tiles input). */
  listOpenTabs(workspaceId: string): { id: string; title: string }[];

  /** Subscribe to source-mode open-tab changes; return unsubscribe. */
  subscribe(workspaceId: string, onChange: () => void): () => void;

  /** 'reparent' strategy: move the live DOM element into `slot`. */
  attach?(tabId: string, slot: HTMLElement): void;

  /** 'reparent' strategy: remove the live DOM element from `slot`. */
  detach?(tabId: string, slot: HTMLElement): void;

  /** 'remount' strategy: mount Svelte content into `slot`; return destroyer. */
  render?(
    tabId: string,
    slot: HTMLElement,
    ctx: { workspaceId: string },
  ): { destroy: () => void };

  /** Metadata for the tile's title bar. Called on each render of the tile. */
  getMeta(tabId: string): TileMeta;

  /** Double-click on title bar → switch to source mode, activate this tab. */
  openInHomeMode(tabId: string): void;

  /** Close the tab — honor source mode's confirm flow if any. */
  closeTab?(tabId: string): void | Promise<void>;
}
