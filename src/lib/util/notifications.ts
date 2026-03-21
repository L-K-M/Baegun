import { writable } from 'svelte/store';
import type { NotificationItem } from '$lib/types';

function createNotificationStore() {
  const { subscribe, update } = writable<NotificationItem[]>([]);
  let nextId = 1;

  return {
    subscribe,
    add(message: string, type: NotificationItem['type'] = 'info', timeoutMs = 3500) {
      const id = nextId++;
      update((items) => [...items, { id, message, type }]);

      if (timeoutMs > 0) {
        setTimeout(() => {
          update((items) => items.filter((item) => item.id !== id));
        }, timeoutMs);
      }
    },
    clear() {
      update(() => []);
    }
  };
}

export const notifications = createNotificationStore();
