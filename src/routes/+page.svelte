<script lang="ts">
  import { onMount } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { Button, Checkbox, Dropdown, ErrorBanner, Notification, ProgressBar, TitleBar } from '@lkmc/system7-ui';

  import type { ConversionJob, ConvertRequest } from '$lib/types';
  import { TauriService } from '$lib/tauri';
  import { notifications } from '$lib/util/notifications';
  import { WindowManager } from '$lib/windowManager';

  const tableFormatOptions = [
    { value: 'html', label: 'HTML tables' },
    { value: 'markdown', label: 'Markdown tables' }
  ];

  let jobs: ConversionJob[] = [];
  let apiKey = '';
  let outputDir = '';
  let model = 'mistral-ocr-latest';
  let tableFormat: 'html' | 'markdown' = 'html';
  let includeImages = true;
  let extractHeader = true;
  let extractFooter = true;
  let validate = false;
  let noCache = false;
  let keepRemoteFile = false;
  let converting = false;
  let progressCurrent = 0;
  let progressTotal = 1;
  let statusMessage = 'Idle';
  let errorMessage = '';
  let windowFocused = true;
  let isWindowShaded = false;

  const appWindow = getCurrentWindow();
  const windowManager = new WindowManager();

  $: pendingCount = jobs.filter((job) => job.status === 'pending').length;
  $: doneCount = jobs.filter((job) => job.status === 'done').length;

  onMount(() => {
    const unlistenFocus = appWindow.onFocusChanged(({ payload }) => {
      windowFocused = payload;
    });

    const unlistenDrop = appWindow.onDragDropEvent((event: any) => {
      if (event?.payload?.type === 'drop') {
        addPaths(event.payload.paths as string[]);
      }
    });

    return () => {
      unlistenFocus.then((fn) => fn());
      unlistenDrop.then((fn) => fn());
    };
  });

  async function choosePdfFiles() {
    const selected = await open({
      multiple: true,
      filters: [{ name: 'PDF', extensions: ['pdf'] }]
    });
    addPaths(normalizeSelection(selected));
  }

  async function chooseOutputDirectory() {
    const selected = await open({
      directory: true,
      multiple: false
    });
    const path = normalizeSelection(selected)[0];
    if (path) {
      outputDir = path;
    }
  }

  function normalizeSelection(selected: string | string[] | null): string[] {
    if (!selected) {
      return [];
    }
    return Array.isArray(selected) ? selected : [selected];
  }

  function addPaths(paths: string[]) {
    const existing = new Set(jobs.map((job) => job.path));
    let added = 0;

    for (const path of paths) {
      if (!/\.pdf$/i.test(path)) {
        continue;
      }
      if (existing.has(path)) {
        continue;
      }
      jobs = [
        ...jobs,
        {
          id: path,
          path,
          status: 'pending'
        }
      ];
      existing.add(path);
      added += 1;
    }

    if (added > 0) {
      notifications.add(`Added ${added} PDF(s) to queue.`, 'info');
    }
  }

  function clearFinished() {
    jobs = jobs.filter((job) => job.status === 'pending' || job.status === 'running');
  }

  async function convertAll() {
    if (converting) {
      return;
    }

    if (!apiKey.trim()) {
      errorMessage = 'Missing API key. Enter one in the API Key field.';
      return;
    }

    const pending = jobs.filter((job) => job.status === 'pending');
    if (pending.length === 0) {
      notifications.add('Queue is empty.', 'info');
      return;
    }

    converting = true;
    errorMessage = '';
    progressCurrent = 0;
    progressTotal = pending.length;
    statusMessage = `Converting ${pending.length} file(s)...`;

    for (const job of pending) {
      updateJob(job.id, { status: 'running', message: undefined });
      statusMessage = `Converting ${basename(job.path)}...`;

      const outputPath = outputDir ? deriveOutputPath(outputDir, job.path) : null;

      const request: ConvertRequest = {
        input_path: job.path,
        output_path: outputPath,
        api_key: apiKey,
        model,
        language: 'en',
        table_format: tableFormat,
        extract_header: extractHeader,
        extract_footer: extractFooter,
        include_images: includeImages,
        no_cache: noCache,
        validate,
        keep_remote_file: keepRemoteFile,
        quiet: true,
        verbose: false
      };

      try {
        const result = await TauriService.convertPdf(request);
        updateJob(job.id, {
          status: 'done',
          outputPath: result.output_path,
          message: `pages=${result.pages_processed}, chapters=${result.chapters}`
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        updateJob(job.id, {
          status: 'error',
          message
        });
      }

      progressCurrent += 1;
    }

    const failures = jobs.filter((job) => job.status === 'error').length;
    const successes = jobs.filter((job) => job.status === 'done').length;

    if (failures > 0) {
      errorMessage = `${failures} conversion(s) failed.`;
      notifications.add(`${successes} succeeded, ${failures} failed.`, 'error');
    } else {
      errorMessage = '';
      notifications.add(`Converted ${successes} file(s).`, 'success');
    }

    statusMessage = 'Idle';
    converting = false;
  }

  function updateJob(id: string, patch: Partial<ConversionJob>) {
    jobs = jobs.map((job) => (job.id === id ? { ...job, ...patch } : job));
  }

  function basename(path: string): string {
    return path.split(/[\\/]/).pop() ?? path;
  }

  function deriveOutputPath(dir: string, inputPath: string): string {
    const file = basename(inputPath).replace(/\.pdf$/i, '') || 'output';
    const separator = dir.includes('\\') ? '\\' : '/';
    const normalizedDir = dir.replace(/[\\/]+$/, '');
    return `${normalizedDir}${separator}${file}.epub`;
  }

  function handleWindowClose() {
    void windowManager.close();
  }

  function handleWindowDrag() {
    void windowManager.startDragging();
  }

  async function handleWindowShade() {
    isWindowShaded = await windowManager.toggleShade();
  }
</script>

<div class="window-frame s7-root" class:window-unfocused={!windowFocused}>
  <TitleBar
    title="Baegun"
    focused={windowFocused}
    closable
    shadeable
    draggable
    onclose={handleWindowClose}
    onshade={handleWindowShade}
    ondragstart={handleWindowDrag}
  />

  <Notification notifications={$notifications} />

  {#if errorMessage}
    <ErrorBanner message={errorMessage} onclose={() => (errorMessage = '')} />
  {/if}

  {#if !isWindowShaded}
    <main class="app-content">
      <section class="settings-panel">
        <h2>Settings</h2>

        <label>
          API Key
          <input type="password" bind:value={apiKey} placeholder="MISTRAL_API_KEY" />
        </label>

        <label>
          OCR Model
          <input type="text" bind:value={model} />
        </label>

        <label>
          Output Directory (optional)
          <div class="path-row">
            <input type="text" bind:value={outputDir} placeholder="same folder as input" />
            <Button onclick={chooseOutputDirectory}>Choose</Button>
          </div>
        </label>

        <div class="setting-row">
          <span>Table Format</span>
          <Dropdown options={tableFormatOptions} bind:value={tableFormat} />
        </div>

        <div class="check-grid">
          <Checkbox
            checked={includeImages}
            label="Include images"
            onchange={(checked: boolean) => (includeImages = checked)}
          />
          <Checkbox
            checked={extractHeader}
            label="Extract header"
            onchange={(checked: boolean) => (extractHeader = checked)}
          />
          <Checkbox
            checked={extractFooter}
            label="Extract footer"
            onchange={(checked: boolean) => (extractFooter = checked)}
          />
          <Checkbox checked={validate} label="Run epubcheck" onchange={(checked: boolean) => (validate = checked)} />
          <Checkbox checked={noCache} label="Disable cache" onchange={(checked: boolean) => (noCache = checked)} />
          <Checkbox
            checked={keepRemoteFile}
            label="Keep remote file"
            onchange={(checked: boolean) => (keepRemoteFile = checked)}
          />
        </div>

        <div class="actions">
          <Button onclick={choosePdfFiles}>Add PDFs</Button>
          <Button onclick={convertAll} disabled={converting || pendingCount === 0}>Convert All</Button>
          <Button onclick={clearFinished} disabled={doneCount === 0}>Clear Finished</Button>
        </div>

        <div class="progress-wrap">
          <ProgressBar value={progressCurrent} max={progressTotal} height={16} ariaLabel="Conversion progress" />
          <span>{statusMessage}</span>
        </div>
      </section>

      <section class="queue-panel">
        <h2>Queue</h2>
        <button class="drop-zone" onclick={choosePdfFiles}>
          Drop PDFs from Finder into this window, or click to browse.
        </button>

        <div class="queue-list" role="list">
          {#if jobs.length === 0}
            <p class="empty">No files queued.</p>
          {:else}
            {#each jobs as job (job.id)}
              <div class="queue-row" role="listitem">
                <div class="queue-main">
                  <span class="name">{basename(job.path)}</span>
                  <span class="status status-{job.status}">{job.status}</span>
                </div>
                {#if job.outputPath}
                  <div class="meta">{job.outputPath}</div>
                {/if}
                {#if job.message}
                  <div class="meta">{job.message}</div>
                {/if}
              </div>
            {/each}
          {/if}
        </div>
      </section>
    </main>
  {/if}
</div>

<style>
  .window-frame {
    width: 100vw;
    height: 100vh;
    background: #fff;
    border: 1px solid #000;
    box-shadow: 2px 2px 0 rgba(0, 0, 0, 0.2);
    display: flex;
    flex-direction: column;
  }

  .app-content {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(320px, 440px) minmax(420px, 1fr);
    gap: 12px;
    padding: 12px;
  }

  .settings-panel,
  .queue-panel {
    border: 1px solid #000;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }

  h2 {
    margin: 0;
    font-size: 18px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  input[type='text'],
  input[type='password'] {
    border: 1px solid #000;
    padding: 6px 8px;
    font-family: inherit;
  }

  .path-row {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px;
  }

  .setting-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .check-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px 8px;
  }

  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .progress-wrap {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .drop-zone {
    border: 2px dashed #000;
    background: #f6f6f6;
    padding: 18px 10px;
    text-align: center;
    cursor: pointer;
    font-family: inherit;
  }

  .queue-list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid #000;
  }

  .queue-row {
    border-bottom: 1px solid #000;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .queue-main {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }

  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .status {
    text-transform: uppercase;
    font-size: 12px;
  }

  .status-pending {
    color: #555;
  }

  .status-running {
    color: #1e5bb8;
  }

  .status-done {
    color: #1b7a3a;
  }

  .status-error {
    color: #b32020;
  }

  .meta {
    font-size: 12px;
    color: #333;
    overflow-wrap: anywhere;
  }

  .empty {
    margin: 0;
    padding: 10px;
    color: #666;
  }

  @media (max-width: 980px) {
    .app-content {
      grid-template-columns: 1fr;
      overflow: auto;
    }

    .check-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
