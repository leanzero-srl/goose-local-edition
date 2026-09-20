export interface BenchmarkRuntimeStatus {
  state: 'missing' | 'ready' | 'unsupported' | 'invalid';
  downloadBytes: number;
  error?: string;
}
export interface BenchmarkRuntimeProgress {
  phase: 'downloading' | 'extracting' | 'verifying';
  receivedBytes?: number;
  totalBytes?: number;
}
