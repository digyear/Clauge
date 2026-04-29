import { writable } from 'svelte/store';
import { TOAST_DURATION_MS } from '$lib/shared/constants/timings';

export type ToastType = 'success' | 'error' | 'info';

export interface ToastItem {
  id: number;
  message: string;
  type: ToastType;
}

let nextId = 0;

export const toasts = writable<ToastItem[]>([]);

export function showToast(message: string, type: ToastType = 'info'): void {
  const id = nextId++;
  toasts.update((t) => [...t, { id, message, type }]);
  setTimeout(() => {
    toasts.update((t) => t.filter((item) => item.id !== id));
  }, TOAST_DURATION_MS);
}
