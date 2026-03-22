import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { ConvertProgressEvent, ConvertRequest, ConvertResponse } from './types';

const CONVERT_PROGRESS_EVENT = 'baegun://convert-progress';

export class TauriService {
  static async convertPdf(request: ConvertRequest): Promise<ConvertResponse> {
    return await invoke('convert_pdf', { request });
  }

  static async listenConvertProgress(
    handler: (payload: ConvertProgressEvent) => void
  ): Promise<UnlistenFn> {
    return await listen<ConvertProgressEvent>(CONVERT_PROGRESS_EVENT, (event) => {
      handler(event.payload);
    });
  }

  static async isDirectory(path: string): Promise<boolean> {
    return await invoke('is_directory', { path });
  }
}
