import type { Download, Filter } from "./types";

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** unit;
  return `${value.toLocaleString(undefined, { maximumFractionDigits: unit === 0 || value >= 100 ? 0 : 1 })} ${units[unit]}`;
}

export function formatEta(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds)) return "—";
  if (seconds < 1) return "< 1 sec";
  if (seconds < 60) return `${Math.ceil(seconds)} sec`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ${Math.floor(seconds % 60)} sec`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} hr ${Math.floor((seconds % 3600) / 60)} min`;
  return `${Math.floor(seconds / 86400)} days`;
}

export function progressPercent(download: Download): number | null {
  if (download.status === "completed") return 100;
  if (download.totalBytes === null || download.totalBytes <= 0) return null;
  return Math.min(100, Math.max(0, (download.downloadedBytes / download.totalBytes) * 100));
}

export function isActive(download: Download): boolean {
  return download.status === "queued" || download.status === "downloading";
}

export function filterDownloads(downloads: Download[], filter: Filter, query: string): Download[] {
  const search = query.trim().toLocaleLowerCase();
  return downloads.filter((download) => {
    const matchesFilter = filter === "all" || (filter === "active" ? isActive(download) : download.status === filter);
    return matchesFilter && (!search || download.fileName.toLocaleLowerCase().includes(search) || download.url.toLocaleLowerCase().includes(search));
  }).sort((a, b) => b.createdAt - a.createdAt || a.queueOrder - b.queueOrder);
}

export function sourceHost(url: string): string {
  try { return new URL(url).hostname; } catch { return url; }
}

export function validateUrl(input: string): string | undefined {
  if (!input.trim()) return "Enter a download URL.";
  try {
    const url = new URL(input.trim());
    if (!/^https?:$/.test(url.protocol) || !url.hostname) return "Use a complete HTTP or HTTPS URL.";
    if (url.username || url.password) return "URLs containing a username or password are not supported.";
  } catch { return "Use a complete HTTP or HTTPS URL."; }
}

export function validateFileName(name: string): string | undefined {
  if (name === "") return;
  if (!name.trim() || name === "." || name === "..") return "Enter a valid filename or leave it blank.";
  if (/[<>:"/\\|?*\u0000-\u001f\u007f-\u009f]/.test(name)) return 'The filename cannot contain control characters or < > : " / \\ | ? *';
  if (/[ .]$/.test(name)) return "The filename cannot end with a space or dot.";
  const stem = name.split(".")[0].trimEnd();
  if (/^(CON|PRN|AUX|NUL|CONIN\$|CONOUT\$|COM[1-9¹²³]|LPT[1-9¹²³])$/i.test(stem)) return "This filename is reserved by Windows. Choose another name.";
  if (name.length > 220 || new TextEncoder().encode(name).length > 220) return "The filename is too long. Use at most 220 bytes / UTF-16 units.";
}
