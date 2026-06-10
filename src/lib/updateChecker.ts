import { invoke } from '@tauri-apps/api/core';

/**
 * Lightweight in-app update check. Asks the Rust `check_for_update` command (which
 * queries GitHub) whether a newer release exists, and — if so — surfaces a dismissible
 * notice that links to the GitHub release page. It never downloads or installs anything.
 *
 * Reusable across the Tauri apps: this file + `UpdateNotice.svelte` are app-agnostic;
 * the repo to check lives in the Rust `updates.rs` (`OWNER`/`REPO`).
 */
export interface UpdateInfo {
  version: string;
  url: string;
  notes: string | null;
}

const SKIP_KEY = 'updateChecker.skippedVersion';
const LAST_CHECK_KEY = 'updateChecker.lastCheck';
const ONE_DAY_MS = 24 * 60 * 60 * 1000;

/**
 * Returns an update to show, or `null`. Throttled to once a day and silenced for a
 * version the user chose to skip. Network/availability failures resolve to `null`
 * (the notice simply doesn't appear). Pass `{ force: true }` for a manual "check now"
 * that ignores the throttle and the skip list.
 */
export async function checkForUpdate({ force = false } = {}): Promise<UpdateInfo | null> {
  if (!force) {
    const last = Number(localStorage.getItem(LAST_CHECK_KEY) ?? '0');
    if (Number.isFinite(last) && Date.now() - last < ONE_DAY_MS) return null;
  }

  let info: UpdateInfo | null;
  try {
    info = await invoke<UpdateInfo | null>('check_self_update');
  } catch (error) {
    console.warn('Update check failed:', error);
    return null; // leave lastCheck untouched so the next launch retries
  }
  localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));

  if (!info) return null;
  if (!force && localStorage.getItem(SKIP_KEY) === info.version) return null;
  return info;
}

/** Don't surface this version again (the notice's "Skip" action). */
export function skipVersion(version: string): void {
  localStorage.setItem(SKIP_KEY, version);
}

/** Open the release page in the user's default browser (via the bundled Rust command). */
export async function openReleasePage(url: string): Promise<void> {
  try {
    await invoke('open_release_url', { url });
  } catch (error) {
    console.warn('Failed to open release page:', error);
  }
}
