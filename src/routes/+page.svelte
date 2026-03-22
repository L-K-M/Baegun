<script lang="ts">
  import { onMount } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { openPath, openUrl } from '@tauri-apps/plugin-opener';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import {
    BalloonHelp,
    Button,
    Checkbox,
    DataTable,
    DownloadIcon,
    ErrorBanner,
    ModalDialog,
    Notification,
    PdfFileIcon,
    ProgressBar,
    TitleBar
  } from '@lkmc/system7-ui';

  import type { ConversionJob, ConvertRequest } from '$lib/types';
  import { TauriService } from '$lib/tauri';
  import { notifications } from '$lib/util/notifications';
  import { WindowManager } from '$lib/windowManager';

  type SortColumn = 'file' | 'status' | 'output' | 'detail';

  const tableColumns = [
    { key: 'file', label: 'File', width: '34%', sortable: true },
    { key: 'status', label: 'Status', width: '12%', sortable: true },
    { key: 'output', label: 'Output', width: '32%', sortable: true },
    { key: 'detail', label: 'Details', width: '22%', sortable: true }
  ];

  const MISTRAL_API_KEYS_URL = 'https://console.mistral.ai/home?profile_dialog=api-keys';

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
  let showSettingsDialog = false;
  let outputDirDropArmed = false;
  let sortColumn: SortColumn = 'file';
  let sortDirection: 'asc' | 'desc' = 'asc';

  const appWindow = getCurrentWindow();
  const windowManager = new WindowManager();

  $: pendingCount = jobs.filter((job) => job.status === 'pending').length;
  $: doneCount = jobs.filter((job) => job.status === 'done').length;
  $: sortedJobs = [...jobs].sort((left, right) => compareJobs(left, right, sortColumn, sortDirection));
  $: missingApiKey = apiKey.trim().length === 0;
  $: missingOutputDir = outputDir.trim().length === 0;
  $: convertDisabled = converting || pendingCount === 0 || missingApiKey || missingOutputDir;
  $: openTargetDisabled = doneCount === 0 || missingOutputDir;
  $: convertDisabledMessage = [
    missingApiKey ? 'Set the API key in Settings first.' : '',
    missingOutputDir ? 'Choose an output directory first.' : '',
    !missingApiKey && !missingOutputDir && pendingCount === 0 ? 'Add at least one PDF first.' : ''
  ]
    .filter(Boolean)
    .join('\n');
  $: openTargetDisabledMessage = [
    missingOutputDir ? 'Choose an output directory first.' : '',
    !missingOutputDir && doneCount === 0 ? 'Convert at least one PDF first.' : ''
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
        if (!outputDirDropArmed) {
          isDropActive = true;
        }
        return;
      }

      if (eventType === 'leave') {
        isDropActive = false;
        outputDirDropArmed = false;
        return;
      }

      if (eventType === 'drop') {
        const droppedPaths = (event.payload.paths as string[]) ?? [];
        const droppedOnOutputDirectory = outputDirDropArmed;

        isDropActive = false;
        outputDirDropArmed = false;

        if (droppedOnOutputDirectory) {
          void setOutputDirectoryFromDrop(droppedPaths);
          return;
        }

        addPaths(droppedPaths);
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

  function openSettingsDialog() {
    showSettingsDialog = true;
  }

  function closeSettingsDialog() {
    showSettingsDialog = false;
  }

  async function openMistralApiKeysPage() {
    try {
      await openUrl(MISTRAL_API_KEYS_URL);
    } catch {
      notifications.add('Unable to open the Mistral API keys page.', 'error');
    }
  }

  async function openTargetFolder() {
    if (missingOutputDir || doneCount === 0) {
      return;
    }

    try {
      await openPath(outputDir.trim());
    } catch {
      notifications.add('Unable to open the target folder.', 'error');
    }
  }

  async function setOutputDirectoryFromDrop(paths: string[]) {
    for (const path of paths) {
      try {
        const isDirectory = await TauriService.isDirectory(path);
        if (!isDirectory) {
          continue;
        }

        outputDir = path;
        notifications.add('Output directory set from dropped folder.', 'info');
        return;
      } catch {
        continue;
      }
    }

    addPaths(paths);
  }

  function handleOutputDirectoryDragEnter(event: DragEvent) {
    event.preventDefault();
    outputDirDropArmed = true;
    isDropActive = false;
  }

  function handleOutputDirectoryDragOver(event: DragEvent) {
    event.preventDefault();
    outputDirDropArmed = true;
    isDropActive = false;
  }

  function handleOutputDirectoryDragLeave(event: DragEvent) {
    event.preventDefault();
    const target = event.currentTarget as HTMLElement | null;
    const next = event.relatedTarget as Node | null;

    if (!target || !next || !target.contains(next)) {
      outputDirDropArmed = false;
    }
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
      errorMessage = 'Missing API key. Open Settings and enter your Mistral key.';
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

  function sortBy(column: SortColumn) {
    if (sortColumn === column) {
      sortDirection = sortDirection === 'asc' ? 'desc' : 'asc';
      return;
    }

    sortColumn = column;
    sortDirection = 'asc';
  }

  function handleTableSort(column: string) {
    if (column === 'file' || column === 'status' || column === 'output' || column === 'detail') {
      sortBy(column);
    }
  }

  function compareJobs(
    left: ConversionJob,
    right: ConversionJob,
    column: SortColumn,
    direction: 'asc' | 'desc'
  ): number {
    const multiplier = direction === 'asc' ? 1 : -1;

    if (column === 'status') {
      const statusRank: Record<ConversionJob['status'], number> = {
        pending: 0,
        running: 1,
        done: 2,
        error: 3
      };
      const result = (statusRank[left.status] - statusRank[right.status]) * multiplier;
      return result || compareText(basename(left.path), basename(right.path), multiplier);
    }

    const leftText = getSortText(left, column);
    const rightText = getSortText(right, column);
    const result = compareText(leftText, rightText, multiplier);
    return result || compareText(basename(left.path), basename(right.path), 1);
  }

  function getSortText(job: ConversionJob, column: SortColumn): string {
    if (column === 'file') {
      return basename(job.path);
    }
    if (column === 'output') {
      return job.outputPath || '';
    }
    return job.message || '';
  }

  function compareText(left: string, right: string, multiplier: number): number {
    return left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' }) * multiplier;
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
      <section class="file-panel">
        <div class="panel-header">
          <div class="header-actions">
            <Button onclick={choosePdfFiles}>Add PDFs</Button>
            <Button onclick={clearFinished} disabled={doneCount === 0}>Clear Finished</Button>
          </div>
          <Button onclick={openSettingsDialog}>Settings...</Button>
        </div>

        <div class="file-table">
          <DataTable
            columns={tableColumns}
            sortKey={sortColumn}
            sortDirection={sortDirection}
            onSort={handleTableSort}
            empty={jobs.length === 0}
            emptyText="Drop PDFs into this table, or click Add PDFs."
            emptyColspan={4}
            bodyClass={isDropActive ? 'drop-active' : ''}
          >
            {#each sortedJobs as job (job.id)}
              <tr>
                <td class="col-file">
                  <div class="file-cell">
                    <span class="file-icon" aria-hidden="true"><PdfFileIcon alt="" size={16} /></span>
                    <span class="file-name" title={basename(job.path)}>{basename(job.path)}</span>
                  </div>
                </td>
                <td class="col-status">
                  <span class="status status-{job.status}">{job.status}</span>
                </td>
                <td class="col-output">{job.outputPath || '-'}</td>
                <td class="col-detail">{job.message || '-'}</td>
              </tr>
            {/each}
          </DataTable>
        </div>
      </section>

      <section class="settings-panel">
        <div class="settings-row">
          <label class="output-dir-field">
            Output directory
            <div
              class="path-row output-drop-target"
              class:drop-armed={outputDirDropArmed}
              role="group"
              aria-label="Output directory drop target"
              ondragenter={handleOutputDirectoryDragEnter}
              ondragleave={handleOutputDirectoryDragLeave}
              ondragover={handleOutputDirectoryDragOver}
            >
              <input type="text" bind:value={outputDir} placeholder="/Users/name/Books" />
              <Button onclick={chooseOutputDirectory}>Choose</Button>
            </div>
          </label>
        </div>

        <div class="settings-footer">
          <div class="queue-meta">Pending: {pendingCount} · Done: {doneCount}</div>
          <div class="settings-footer-actions">
            <BalloonHelp message={openTargetDisabled ? openTargetDisabledMessage : ''} position="top" delay={250}>
              <span>
                <Button onclick={openTargetFolder} disabled={openTargetDisabled}>Open Target Folder</Button>
              </span>
            </BalloonHelp>
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
        </div>
      </section>
    </main>
  {/if}

  {#if showSettingsDialog}
    <ModalDialog width="500px" onclose={closeSettingsDialog}>
      <div class="settings-dialog">
        <h3>Settings</h3>
        <label>
          Mistral API key
          <input type="password" bind:value={apiKey} placeholder="ufuVDexxxxxxxxxxxxxxxxxxxxxxxx" />
        </label>
        <div class="settings-dialog-meta">
          <p class="settings-dialog-hint">This key is used for OCR requests to Mistral.</p>
          <button type="button" class="settings-link" onclick={openMistralApiKeysPage}>
            Open Mistral API keys ->
          </button>
        </div>
        <div class="settings-dialog-toggle-row">
          <Checkbox
            checked={includeImages}
            label="Include images"
            onchange={(checked: boolean) => (includeImages = checked)}
          />
          <Checkbox checked={validate} label="Run epubcheck" onchange={(checked: boolean) => (validate = checked)} />
        </div>
        <div class="settings-dialog-actions">
          <Button variant="primary" onclick={closeSettingsDialog}>Done</Button>
        </div>
      </div>
    </ModalDialog>
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

  h3 {
    margin: 0;
    font-weight: 700;
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
  }

  .path-row {
    display: grid;
    grid-template-columns: minmax(220px, 1fr) auto;
    gap: 6px;
    align-items: center;
  }

  .output-drop-target.drop-armed {
    outline: 2px dashed #365ea8;
    outline-offset: 2px;
    background: #eef4ff;
  }

  .settings-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }

  .output-dir-field {
    flex: 1 1 420px;
    min-width: 260px;
  }

  .settings-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .settings-footer-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .queue-meta {
    color: #444;
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
    width: 100%;
    border: 1px solid #000;
  }

  .file-table :global(.s7-data-table-body-container.drop-active) {
    background: #eef4ff;
    outline: 2px dashed #365ea8;
    outline-offset: -3px;
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

  .file-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-output,
  .col-detail {
    overflow-wrap: anywhere;
  }

  .status {
    text-transform: uppercase;
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

  .progress-modal {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .progress-modal p {
    margin: 0;
  }

  .modal-meta {
    color: #333;
  }

  .settings-dialog {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .settings-dialog-hint {
    margin: 0;
    color: #444;
  }

  .settings-dialog-meta {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .settings-link {
    align-self: flex-start;
    border: none;
    background: transparent;
    padding: 0;
    font: inherit;
    color: #000;
    text-decoration: underline;
    cursor: pointer;
  }

  .settings-link:hover {
    text-decoration: none;
  }

  .settings-dialog-toggle-row {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    align-items: center;
  }

  .settings-dialog-actions {
    display: flex;
    justify-content: flex-end;
  }

  @media (max-width: 900px) {
    .settings-footer,
    .settings-footer-actions,
    .settings-row,
    .panel-header,
    .header-actions,
    .settings-dialog-toggle-row {
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
