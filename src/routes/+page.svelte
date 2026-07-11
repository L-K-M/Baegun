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
    TextInput,
    TrashIcon,
    TitleBar,
    getSystem7WindowStyle
  } from '@lkmc/system7-ui';

  import type { ConversionJob, ConvertProgressEvent, ConvertRequest, SystemColors } from '$lib/types';
  import { TauriService } from '$lib/tauri';
  import { notifications } from '$lib/util/notifications';
  import { WindowManager } from '$lib/windowManager';

  type SortColumn = 'file' | 'status' | 'output' | 'detail';

  const tableColumns = [
    { key: 'file', label: 'File', width: '32%', sortable: true },
    { key: 'status', label: 'Status', width: '12%', sortable: true },
    { key: 'output', label: 'Output', width: '28%', sortable: true },
    { key: 'detail', label: 'Details', width: '22%', sortable: true },
    { key: 'remove', label: '', width: '6%', align: 'right' as const, sortable: false }
  ];

  const MISTRAL_API_KEYS_URL = 'https://console.mistral.ai/home?profile_dialog=api-keys';
  const SETTINGS_STORAGE_KEY = 'baegun.desktop.settings.v1';

  type PersistedSettings = {
    apiKey: string;
    outputDir: string;
    includeImages: boolean;
    comicMode: boolean;
    validate: boolean;
  };

  const STAGE_LABELS: Record<ConvertProgressEvent['stage'], string> = {
    reading_input: 'Input',
    ocr: 'OCR',
    normalize: 'Normalize',
    package_epub: 'Package',
    validate: 'Validate',
    complete: 'Complete'
  };

  let jobs: ConversionJob[] = [];
  let apiKey = '';
  let outputDir = '';
  let includeImages = true;
  let comicMode = false;
  let validate = false;

  let converting = false;
  let systemColors: SystemColors | null = null;
  let showProgressModal = false;
  let progressCurrent = 0;
  let progressTotal = 1;
  let stageStep = 0;
  let stageTotal = 0;
  let statusMessage = 'Idle';
  let errorMessage = '';
  let windowFocused = true;
  let isWindowShaded = false;
  let isDropActive = false;
  let showSettingsDialog = false;
  let outputDirDropArmed = false;
  let activeJobPath: string | null = null;
  let cancelRequested = false;
  let settingsLoaded = false;
  let sortColumn: SortColumn = 'file';
  let sortDirection: 'asc' | 'desc' = 'asc';

  const appWindow = getCurrentWindow();
  const windowManager = new WindowManager();

  $: pendingCount = jobs.filter((job) => job.status === 'pending').length;
  $: doneCount = jobs.filter((job) => job.status === 'done').length;
  $: finishedCount = jobs.filter((job) => job.status === 'done' || job.status === 'error').length;
  $: sortedJobs = [...jobs].sort((left, right) => compareJobs(left, right, sortColumn, sortDirection));
  $: missingApiKey = apiKey.trim().length === 0;
  $: pendingPdfCount = jobs.filter((job) => job.status === 'pending' && isPdf(job.path)).length;
  $: pendingPdfNeedsApiKey = missingApiKey && pendingPdfCount > 0;
  $: missingOutputDir = outputDir.trim().length === 0;
  $: convertDisabled = converting || pendingCount === 0 || pendingPdfNeedsApiKey || missingOutputDir;
  $: openTargetDisabled = doneCount === 0 || missingOutputDir;
  $: convertDisabledMessage = [
    pendingPdfNeedsApiKey ? 'Set the API key in Settings for pending PDFs.' : '',
    missingOutputDir ? 'Choose an output directory first.' : '',
    !pendingPdfNeedsApiKey && !missingOutputDir && pendingCount === 0 ? 'Add at least one book first.' : ''
  ]
    .filter(Boolean)
    .join('\n');
  $: openTargetDisabledMessage = [
    missingOutputDir ? 'Choose an output directory first.' : '',
    !missingOutputDir && doneCount === 0 ? 'Convert at least one book first.' : ''
  ]
    .filter(Boolean)
    .join('\n');

  $: if (settingsLoaded) {
    persistSettings({
      apiKey,
      outputDir,
      includeImages,
      comicMode,
      validate
    });
  }

  $: windowStyle = systemColors ? getSystem7WindowStyle(systemColors) : '';

  function handleConvertProgress(progress: ConvertProgressEvent) {
    if (!converting || !activeJobPath) {
      return;
    }

    if (progress.input_path !== activeJobPath) {
      return;
    }

    const stageLabel = STAGE_LABELS[progress.stage] ?? 'Progress';
    stageStep = progress.step;
    stageTotal = progress.total_steps;
    const cancelSuffix = cancelRequested ? ' (cancel requested)' : '';
    statusMessage = `${basename(progress.input_path)} · ${stageLabel}: ${progress.message}${cancelSuffix}`;
  }

  function loadPersistedSettings() {
    const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
    if (!raw) {
      return;
    }

    try {
      const parsed = JSON.parse(raw) as Partial<PersistedSettings>;

      if (typeof parsed.apiKey === 'string') {
        apiKey = parsed.apiKey;
      }

      if (typeof parsed.outputDir === 'string') {
        outputDir = parsed.outputDir;
      }

      if (typeof parsed.includeImages === 'boolean') {
        includeImages = parsed.includeImages;
      }

      if (typeof parsed.comicMode === 'boolean') {
        comicMode = parsed.comicMode;
      }

      if (typeof parsed.validate === 'boolean') {
        validate = parsed.validate;
      }
    } catch {
      localStorage.removeItem(SETTINGS_STORAGE_KEY);
    }
  }

  function persistSettings(settings: PersistedSettings) {
    localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings));
  }

  onMount(() => {
    loadPersistedSettings();
    settingsLoaded = true;
    void loadSystemColors();

    const unlistenFocus = appWindow.onFocusChanged(({ payload }) => {
      windowFocused = payload;
    });

    const unlistenProgress = TauriService.listenConvertProgress((progress) => {
      handleConvertProgress(progress);
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
      unlistenProgress.then((fn) => fn());
    };
  });

  async function loadSystemColors() {
    try {
      systemColors = await TauriService.getSystemColors();
    } catch {
      systemColors = null;
    }
  }

  async function chooseBookFiles() {
    const selected = await open({
      multiple: true,
      filters: [{ name: 'Books', extensions: ['pdf', 'cbz'] }]
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
      if (!/\.(pdf|cbz)$/i.test(path)) {
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
      notifications.add(`Added ${added} book(s) to queue.`, 'info');
    }
  }

  function removeJob(id: string) {
    if (converting) {
      return;
    }

    jobs = jobs.filter((job) => job.id !== id);
  }

  function clearFinished() {
    jobs = jobs.filter((job) => job.status === 'pending' || job.status === 'running');
  }

  function handleComicModeChange(checked: boolean) {
    comicMode = checked;
    if (checked) {
      includeImages = true;
    }
  }

  function requestCancelConversion() {
    if (!converting || cancelRequested) {
      return;
    }

    cancelRequested = true;
    if (activeJobPath) {
      statusMessage = `Cancel requested. Finishing ${basename(activeJobPath)}...`;
    } else {
      statusMessage = 'Cancel requested.';
    }
  }

  async function convertAll() {
    if (converting) {
      return;
    }

    if (pendingPdfNeedsApiKey) {
      errorMessage = 'Missing API key. Pending PDFs require a Mistral key; CBZ files do not.';
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
    const runJobIds = new Set(pending.map((job) => job.id));

    converting = true;
    cancelRequested = false;
    showProgressModal = true;
    errorMessage = '';
    progressCurrent = 0;
    progressTotal = pending.length;
    stageStep = 0;
    stageTotal = 0;
    statusMessage = `Converting ${pending.length} file(s)...`;

    // Seed with outputs already produced in this queue so a re-run cannot
    // silently overwrite them with a same-named book from another folder.
    const usedOutputs = new Set(
      jobs
        .filter((job) => job.outputPath)
        .map((job) => (job.outputPath as string).toLowerCase())
    );

    for (const job of pending) {
      if (cancelRequested) {
        break;
      }

      activeJobPath = job.path;
      updateJob(job.id, { status: 'running', message: undefined });
      statusMessage = `Converting ${basename(job.path)}...`;

      const outputPath = deriveOutputPath(outputDir.trim(), job.path, usedOutputs);
      usedOutputs.add(outputPath.toLowerCase());

      const request: ConvertRequest = {
        input_path: job.path,
        output_path: outputPath,
        api_key: apiKey,
        language: 'en',
        include_images: isPdf(job.path) ? includeImages || comicMode : true,
        comic_mode: isPdf(job.path) && comicMode,
        validate,
        quiet: true,
        verbose: false
      };

      try {
        const result = await TauriService.convertBook(request);
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
      stageStep = 0;
      stageTotal = 0;
      activeJobPath = null;
    }

    const canceled = cancelRequested;
    const runJobs = jobs.filter((job) => runJobIds.has(job.id));
    const pendingRemaining = runJobs.filter((job) => job.status === 'pending').length;
    const failures = runJobs.filter((job) => job.status === 'error').length;
    const successes = runJobs.filter((job) => job.status === 'done').length;

    if (canceled) {
      if (failures > 0) {
        errorMessage = `${failures} conversion(s) failed before cancel.`;
        notifications.add(
          `Canceled with ${successes} succeeded, ${failures} failed, ${pendingRemaining} pending.`,
          'error'
        );
      } else {
        notifications.add(`Canceled with ${successes} succeeded and ${pendingRemaining} pending.`, 'info');
      }
    } else if (failures > 0) {
      errorMessage = `${failures} conversion(s) failed.`;
      notifications.add(`${successes} succeeded, ${failures} failed.`, 'error');
    } else {
      notifications.add(`Converted ${successes} file(s).`, 'success');
    }

    activeJobPath = null;
    stageStep = 0;
    stageTotal = 0;
    statusMessage = 'Idle';
    converting = false;
    cancelRequested = false;
    showProgressModal = false;
  }

  function updateJob(id: string, patch: Partial<ConversionJob>) {
    jobs = jobs.map((job) => (job.id === id ? { ...job, ...patch } : job));
  }

  function basename(path: string): string {
    return path.split(/[\\/]/).pop() ?? path;
  }

  function isPdf(path: string): boolean {
    return /\.pdf$/i.test(path);
  }

  function deriveOutputPath(dir: string, inputPath: string, usedOutputs: Set<string>): string {
    const file = basename(inputPath).replace(/\.(pdf|cbz)$/i, '') || 'output';
    const separator = dir.includes('\\') ? '\\' : '/';
    const normalizedDir = dir.replace(/[\\/]+$/, '');

    // Queued books from different folders can share a basename; suffix instead
    // of letting the second conversion overwrite the first EPUB.
    let candidate = `${normalizedDir}${separator}${file}.epub`;
    let counter = 2;
    while (usedOutputs.has(candidate.toLowerCase())) {
      candidate = `${normalizedDir}${separator}${file}-${counter}.epub`;
      counter += 1;
    }
    return candidate;
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

<div class="window-frame s7-root" class:window-unfocused={!windowFocused} style={windowStyle}>
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

  {#if !isWindowShaded}
    <Notification notifications={$notifications} ondismiss={(id) => notifications.remove(id)} />

    {#if errorMessage}
      <ErrorBanner message={errorMessage} onclose={() => (errorMessage = '')} />
    {/if}

    <main class="app-content">
      <section class="file-panel">
        <div class="panel-header">
          <div class="header-actions">
            <Button onclick={chooseBookFiles}>Add Books</Button>
            <Button onclick={clearFinished} disabled={finishedCount === 0}>Clear Finished</Button>
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
            emptyText="Drop PDF or CBZ books into this table, or click Add Books."
            emptyColspan={5}
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
                <td class="col-remove">
                  <Button
                    variant="icon"
                    title={`Remove ${basename(job.path)} from queue`}
                    onclick={() => removeJob(job.id)}
                    disabled={converting}
                  >
                    <TrashIcon alt={`Remove ${basename(job.path)}`} size={16} />
                  </Button>
                </td>
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
              <TextInput bind:value={outputDir} placeholder="/Users/name/Books" ariaLabel="Output directory" />
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
          <TextInput
            type="password"
            bind:value={apiKey}
            clearable
            placeholder="ufuVDexxxxxxxxxxxxxxxxxxxxxxxx"
          />
        </label>
        <div class="settings-dialog-meta">
          <p class="settings-dialog-hint">This key is used for PDF OCR requests to Mistral. CBZ conversion is local.</p>
          <button type="button" class="settings-link" onclick={openMistralApiKeysPage}>
            Open Mistral API keys ->
          </button>
        </div>
        <div class="settings-dialog-toggle-row">
          <Checkbox
            checked={includeImages}
            disabled={comicMode}
            label="Include images"
            onchange={(checked: boolean) => (includeImages = checked)}
          />
          <Checkbox checked={comicMode} label="PDF comic mode" onchange={handleComicModeChange} />
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
        {#if stageTotal > 0}
          <p class="modal-stage">Stage {stageStep} of {stageTotal}</p>
        {/if}
        <p class="modal-meta">{progressCurrent} of {progressTotal} complete</p>
        <div class="progress-modal-actions">
          <Button onclick={requestCancelConversion} disabled={cancelRequested}>
            {cancelRequested ? 'Canceling...' : 'Cancel'}
          </Button>
        </div>
      </div>
    </ModalDialog>
  {/if}
</div>

<style>
  .window-frame {
    /* Monochrome System 7 defaults; the OS accent colors fetched at
       runtime override these via the inline style. */
    --system7-color-accent: #000;
    --system7-color-accent-text: #fff;
    --system7-color-highlight: #000;
    --system7-color-highlight-text: #fff;
    --system7-color-success: #000;
    --system7-color-error: #000;
    --system7-color-info: #000;
    width: 100vw;
    height: 100vh;
    background: #fff;
    border: 1px solid #000;
    box-shadow: 2px 2px 0 rgba(0, 0, 0, 0.2);
    display: flex;
    flex-direction: column;
  }

  .window-frame :global(.notification.success),
  .window-frame :global(.notification.error),
  .window-frame :global(.notification.info) {
    border-left: 2px solid var(--system7-color-ink, #000);
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

  label :global(.sys7-text-input-wrap) {
    width: 100%;
  }

  label :global(.sys7-text-input) {
    width: 100%;
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
    width: 32%;
  }

  .col-status {
    width: 12%;
  }

  .col-output {
    width: 28%;
  }

  .col-detail {
    width: 22%;
  }

  .col-remove {
    width: 6%;
    text-align: right;
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

  .progress-modal-actions {
    display: flex;
    justify-content: flex-end;
  }

  .modal-meta {
    color: #333;
  }

  .modal-stage {
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

    .col-remove {
      width: 10%;
    }
  }
</style>
