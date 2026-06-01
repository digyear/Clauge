import { get } from 'svelte/store';
import {
  viewport,
  interactionState,
  ZOOM_MIN,
  ZOOM_MAX,
  flushViewportSoon,
} from '$lib/modes/canvas/stores/canvasStore';

const ZOOM_IDLE_TIMEOUT_MS = 120;
const ZOOM_DELTA_FACTOR = 0.0015;

/**
 * Svelte action: zoom on ctrl/cmd + wheel, anchored at cursor position.
 * After the gesture settles, flushes viewport state to the backend.
 */
export function zoomable(node: HTMLElement) {
  let zoomIdleTimer: ReturnType<typeof setTimeout> | null = null;

  function onWheel(e: WheelEvent) {
    if (!(e.ctrlKey || e.metaKey)) return; // pan action handles non-ctrl wheels.
    e.preventDefault();
    const rect = node.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    const v = get(viewport);
    const factor = Math.exp(-e.deltaY * ZOOM_DELTA_FACTOR);
    const newZoom = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, v.zoom * factor));
    if (newZoom === v.zoom) return;
    // Zoom around the cursor: adjust offset so the canvas point under the
    // cursor stays fixed.
    const k = newZoom / v.zoom;
    const newOffsetX = px - k * (px - v.offsetX);
    const newOffsetY = py - k * (py - v.offsetY);
    viewport.set({ offsetX: newOffsetX, offsetY: newOffsetY, zoom: newZoom });
    interactionState.set('zooming');
    document.body.classList.add('cv-interacting');
    if (zoomIdleTimer) clearTimeout(zoomIdleTimer);
    zoomIdleTimer = setTimeout(() => {
      interactionState.set('idle');
      document.body.classList.remove('cv-interacting');
      flushViewportSoon();
    }, ZOOM_IDLE_TIMEOUT_MS);
  }

  node.addEventListener('wheel', onWheel, { passive: false });
  return {
    destroy() {
      node.removeEventListener('wheel', onWheel);
      if (zoomIdleTimer) clearTimeout(zoomIdleTimer);
    },
  };
}
