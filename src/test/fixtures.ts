import type { Download, Snapshot } from "../types";

export function download(overrides: Partial<Download> = {}): Download {
  return {
    id: "download-1", url: "https://example.com/archive.zip", fileName: "archive.zip",
    destination: "C:\\Users\\Test\\Downloads", status: "downloading", downloadedBytes: 1024,
    totalBytes: 4096, speedBps: 512, etaSeconds: 6, createdAt: 1_700_000_000_000,
    queueOrder: 0, error: null, connections: 1, ...overrides,
  };
}

export const snapshot: Snapshot = {
  downloads: [], settings: {
    downloadDir: "C:\\Users\\Test\\Downloads", maxConcurrent: 3, connectionsPerDownload: 4,
    speedLimitBps: 0, theme: "system", notifications: true,
  },
};
