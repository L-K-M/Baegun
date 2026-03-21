import { invoke } from '@tauri-apps/api/core';
import type { ConvertRequest, ConvertResponse } from './types';

export class TauriService {
  static async convertPdf(request: ConvertRequest): Promise<ConvertResponse> {
    return await invoke('convert_pdf', { request });
  }

  static async isDirectory(path: string): Promise<boolean> {
    return await invoke('is_directory', { path });
  }
}
