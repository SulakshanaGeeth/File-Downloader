export type DownloadStatus = "queued" | "downloading" | "paused" | "completed" | "failed" | "cancelled";
export interface Download {
  id: string;
  url: string;
  fileName: string;
  destination: string;
  status: DownloadStatus;
  downloadedBytes: number;
  totalBytes: number | null;
  speedBps: number;
  etaSeconds: number | null;
  createdAt: number;
  queueOrder: number;
  error: string | null;
  connections: number;
}
export interface Settings {
  downloadDir: string;
  maxConcurrent: number;
  connectionsPerDownload: number;
  speedLimitBps: number;
  theme: "system" | "light" | "dark";
  notifications: boolean;
}
export interface Snapshot { downloads: Download[]; settings: Settings }
export interface AddRequest { url: string; fileName?: string | null; destination?: string | null }
export type Filter = "all" | "active" | "completed" | "failed";
