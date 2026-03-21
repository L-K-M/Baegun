<script lang="ts">
  import { onMount } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import {
    BalloonHelp,
    Button,
    Checkbox,
    DownloadIcon,
    ErrorBanner,
    ModalDialog,
    Notification,
    ProgressBar,
    TitleBar
  } from '@lkmc/system7-ui';

  import type { ConversionJob, ConvertRequest } from '$lib/types';
  import { TauriService } from '$lib/tauri';
  import { notifications } from '$lib/util/notifications';
  import { WindowManager } from '$lib/windowManager';

  let jobs: ConversionJob[] = [];
  let apiKey = '';
  let outputDir = '';
  let includeImages = true;
  let validate = false;

  let converting = false;
  let showProgressModal = false;
  let progressCurrent = 0;
  let progressTotal = 1;
  let statusMessage = 'Idle';
  let errorMessage = '';
  let windowFocused = true;
  let isWindowShaded = false;
  let isDropActive = false;

  const appWindow = getCurrentWindow();
  const windowManager = new WindowManager();

  $: pendingCount = jobs.filter((job) => job.status === 'pending').length;
  $: doneCount = jobs.filter((job) => job.status === 'done').length;
  $: missingApiKey = apiKey.trim().length === 0;
  $: missingOutputDir = outputDir.trim().length === 0;
  $: convertDisabled = converting || pendingCount === 0 || missingApiKey || missingOutputDir;
  $: convertDisabledMessage = [
    missingApiKey ? 'Set the API key first.' : '',
    missingOutputDir ? 'Choose an output directory first.' : '',
    !missingApiKey && !missingOutputDir && pendingCount === 0 ? 'Add at least one PDF first.' : ''
  ]
    .filter(Boolean)
    .join('\n');

  onMount(() => {
    const unlistenFocus = appWindow.onFocusChanged(({ payload }) => {
      windowFocused = payload;
    });

    const unlistenDrop = appWindow.onDragDropEvent((event: any) => {
      const eventType = event?.payload?.type;

      if (eventType === 'enter' || eventType === 'over') {
        isDropActive = true;
        return;
      }

      if (eventType === 'leave') {
        isDropActive = false;
        return;
      }

      if (eventType === 'drop') {
        isDropActive = false;
        addPaths((event.payload.paths as string[]) ?? []);
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

    if (missingApiKey) {
      errorMessage = 'Missing API key. Enter one in the API key field.';
      return;
    }

    if (missingOutputDir) {
      errorMessage = 'Missing output directory. Choose a directory for EPUB output.';
      return;
    }

    const pending = jobs.filter((job) => job.status === 'pending');
    if (pending.length === 0) {
      notifications.add('Queue is empty.', 'info');
      return;
    }

    converting = true;
    showProgressModal = true;
    errorMessage = '';
    progressCurrent = 0;
    progressTotal = pending.length;
    statusMessage = `Converting ${pending.length} file(s)...`;

    for (const job of pending) {
      updateJob(job.id, { status: 'running', message: undefined });
      statusMessage = `Converting ${basename(job.path)}...`;

      const outputPath = deriveOutputPath(outputDir.trim(), job.path);

      const request: ConvertRequest = {
        input_path: job.path,
        output_path: outputPath,
        api_key: apiKey,
        language: 'en',
        include_images: includeImages,
        validate,
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
      notifications.add(`Converted ${successes} file(s).`, 'success');
    }

    statusMessage = 'Idle';
    converting = false;
    showProgressModal = false;
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
    windowManager.close();
  }

  function handleWindowDrag() {
    windowManager.startDragging();
  }

  async function handleWindowShade() {
    isWindowShaded = await windowManager.toggleShade();
  }
</script>

<div class="window-frame" class:window-unfocused={!windowFocused}>
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
      <section class="file-panel">
        <div class="panel-header">
          <h2>File List</h2>
          <div class="header-actions">
            <Button onclick={choosePdfFiles}>Add PDFs</Button>
            <Button onclick={clearFinished} disabled={doneCount === 0}>Clear Finished</Button>
          </div>
        </div>

        <div class="file-table">
          <div class="table-header-container">
            <table>
              <thead>
                <tr>
                  <th class="col-file">File</th>
                  <th class="col-status">Status</th>
                  <th class="col-output">Output</th>
                  <th class="col-detail">Details</th>
                </tr>
              </thead>
            </table>
          </div>

          <div class="table-body-container" class:drop-active={isDropActive}>
            <table>
              <tbody>
                {#if jobs.length === 0}
                  <tr>
                    <td colspan="4" class="empty-row">
                      Drop PDFs into this table, or click <strong>Add PDFs</strong>.
                    </td>
                  </tr>
                {:else}
                  {#each jobs as job (job.id)}
                    <tr>
                      <td class="col-file">
                        <div class="file-cell">
                          <span class="file-icon" aria-hidden="true">📄</span>
                          <span class="file-name">{basename(job.path)}</span>
                        </div>
                      </td>
                      <td class="col-status">
                        <span class="status status-{job.status}">{job.status}</span>
                      </td>
                      <td class="col-output">{job.outputPath || '-'}</td>
                      <td class="col-detail">{job.message || '-'}</td>
                    </tr>
                  {/each}
                {/if}
              </tbody>
            </table>
          </div>
        </div>
      </section>

      <section class="settings-panel">
        <h2>Settings</h2>

        <div class="credentials-row">
          <label>
            API key
            <input type="password" bind:value={apiKey} placeholder="mistral-xxxxxxxxxxxxxxxxxxxxxxxx" />
          </label>

          <label>
            Output directory
            <div class="path-row">
              <input type="text" bind:value={outputDir} placeholder="/Users/name/Books" />
              <Button onclick={chooseOutputDirectory}>Choose</Button>
            </div>
          </label>
        </div>

        <div class="check-row">
          <Checkbox
            checked={includeImages}
            label="Include images"
            onchange={(checked: boolean) => (includeImages = checked)}
          />
          <Checkbox checked={validate} label="Run epubcheck" onchange={(checked: boolean) => (validate = checked)} />
        </div>

        <div class="settings-footer">
          <div class="queue-meta">Pending: {pendingCount} · Done: {doneCount}</div>
          <BalloonHelp message={convertDisabled ? convertDisabledMessage : ''} position="top" delay={250}>
            <span>
              <Button variant="primary" onclick={convertAll} disabled={convertDisabled}>
                <span class="convert-button-content">
                  <DownloadIcon alt="Convert" size={14} />
                  Convert All
                </span>
              </Button>
            </span>
          </BalloonHelp>
        </div>
      </section>
    </main>
  {/if}

  {#if showProgressModal}
    <ModalDialog width="440px">
      <div class="progress-modal">
        <h3>Converting Files</h3>
        <p>{statusMessage}</p>
        <ProgressBar value={progressCurrent} max={progressTotal} height={16} ariaLabel="Conversion progress" />
        <p class="modal-meta">{progressCurrent} of {progressTotal} complete</p>
      </div>
    </ModalDialog>
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
    font-family: 'Geneva', 'Verdana', 'Helvetica', sans-serif;
    font-size: 14px;
  }

  .window-frame :global(.title-text span) {
    font-size: 18px !important;
  }

  .app-content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
  }

  .file-panel,
  .settings-panel {
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }

  .file-panel {
    flex: 1;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .header-actions {
    display: flex;
    gap: 8px;
  }

  h2,
  h3 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
  }

  input[type='text'],
  input[type='password'] {
    border: 1px solid #000;
    padding: 6px 8px;
    font-family: 'Geneva', 'Verdana', 'Helvetica', sans-serif;
    font-size: 13px;
  }

  .path-row {
    display: grid;
    grid-template-columns: minmax(220px, 1fr) auto;
    gap: 6px;
    align-items: center;
  }

  .credentials-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    align-items: start;
  }

  .check-row {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
  }

  .settings-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .queue-meta {
    color: #444;
    font-size: 12px;
  }

  .convert-button-content {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .file-table {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border: 1px solid #000;
  }

  .table-header-container {
    padding-right: 16px;
    border-bottom: 1px solid #000;
    background: #fff;
  }

  .table-body-container {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    border-top: 1px solid #000;
    margin-top: 2px;
    background: #fff;
  }

  .table-body-container.drop-active {
    background: #eef4ff;
    outline: 2px dashed #365ea8;
    outline-offset: -3px;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
    font-family: 'Geneva', 'Verdana', 'Helvetica', sans-serif;
    font-size: 13px;
  }

  th,
  td {
    text-align: left;
    padding: 5px 8px;
    vertical-align: middle;
    border-bottom: 1px solid #d9d9d9;
  }

  th {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #222;
  }

  tr:last-child td {
    border-bottom: none;
  }

  .col-file {
    width: 34%;
  }

  .col-status {
    width: 12%;
  }

  .col-output {
    width: 32%;
  }

  .col-detail {
    width: 22%;
  }

  .file-cell {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .file-icon {
    width: 16px;
    text-align: center;
  }

  .file-name,
  .col-output,
  .col-detail {
    overflow-wrap: anywhere;
  }

  .status {
    text-transform: uppercase;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.05em;
  }

  .status-pending {
    color: #4f4f4f;
  }

  .status-running {
    color: #1e5bb8;
  }

  .status-done {
    color: #1a7a3b;
  }

  .status-error {
    color: #b32020;
  }

  .empty-row {
    padding: 18px 10px;
    text-align: center;
    color: #666;
    border-bottom: none;
  }

  .progress-modal {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .progress-modal p {
    margin: 0;
    font-size: 13px;
  }

  .modal-meta {
    color: #333;
  }

  @media (max-width: 900px) {
    .credentials-row {
      grid-template-columns: 1fr;
    }

    .settings-footer,
    .panel-header,
    .header-actions,
    .check-row {
      flex-direction: column;
      align-items: flex-start;
    }

    .path-row {
      grid-template-columns: 1fr;
    }

    .col-status,
    .col-detail {
      width: 18%;
    }
  }
</style>
