export type JobStatus = 'pending' | 'running' | 'done' | 'error';

export interface ConversionJob {
  id: string;
  path: string;
  status: JobStatus;
  outputPath?: string;
  message?: string;
}

export interface ConvertRequest {
  input_path: string;
  output_path?: string | null;
  api_key?: string | null;
  model?: string;
  title?: string | null;
  author?: string | null;
  language?: string;
  publisher?: string | null;
  table_format?: 'html' | 'markdown';
  extract_header?: boolean;
  extract_footer?: boolean;
  include_images?: boolean;
  cache_dir?: string;
  no_cache?: boolean;
  validate?: boolean;
  epubcheck_bin?: string;
  keep_remote_file?: boolean;
  fail_on_warn?: boolean;
  debug_dir?: string | null;
  quiet?: boolean;
  verbose?: boolean;
}

export interface ConvertResponse {
  output_path: string;
  pages_processed: number;
  chapters: number;
  images: number;
  cache_hit: boolean;
  validation_warnings: number;
  validation_errors: number;
}

export interface NotificationItem {
  id: number;
  message: string;
  type: 'success' | 'error' | 'info';
}
