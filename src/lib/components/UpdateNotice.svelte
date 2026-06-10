<script lang="ts">
  import { onMount } from 'svelte';
  import { Button, CloseIcon } from '@lkmc/system7-ui';
  import { checkForUpdate, skipVersion, openReleasePage, type UpdateInfo } from '$lib/updateChecker';

  // The update to surface, or null when up to date / dismissed.
  let update = $state<UpdateInfo | null>(null);

  onMount(async () => {
    update = await checkForUpdate();
  });

  function view() {
    if (update) openReleasePage(update.url);
  }

  function skip() {
    if (update) skipVersion(update.version);
    update = null;
  }

  function dismiss() {
    update = null;
  }
</script>

{#if update}
  <div class="update-notice" role="status" aria-live="polite">
    <button class="dismiss" type="button" onclick={dismiss} aria-label="Dismiss this reminder">
      <CloseIcon size={14} />
    </button>
    <p class="message">A new version (<strong>{update.version}</strong>) is available.</p>
    <div class="actions">
      <Button onclick={skip}>Skip This Version</Button>
      <Button variant="primary" onclick={view}>View on GitHub</Button>
    </div>
  </div>
{/if}

<style>
  /* Matches @lkmc/system7-ui's Notification chrome: paper fill, ink border, hard
     drop shadow, and its design tokens — so it reads as a System 7 alert. */
  .update-notice {
    position: fixed;
    right: 20px;
    bottom: 20px;
    z-index: var(--system7-z-notification, 1000);
    width: 300px;
    max-width: calc(100vw - 40px);
    padding: 14px 16px 12px;
    background: var(--system7-color-paper, #fff);
    color: var(--system7-color-ink, #000);
    border: 2px solid var(--system7-color-ink, #000);
    border-radius: 10px;
    box-shadow: 2px 2px 0 var(--system7-shadow-color, #000);
  }

  .message {
    margin: 0 0 12px;
    padding-right: 16px; /* clears the close button */
    font-size: 13px;
    line-height: 1.4;
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
  }

  .dismiss {
    position: absolute;
    top: 6px;
    right: 8px;
    padding: 0;
    background: none;
    border: none;
    line-height: 0;
    cursor: default;
  }
</style>
