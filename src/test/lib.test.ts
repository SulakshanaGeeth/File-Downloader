import { describe, expect, it } from "vitest";
import { filterDownloads, formatBytes, formatEta, progressPercent, validateFileName, validateUrl } from "../lib";
import { download } from "./fixtures";

describe("download presentation", () => {
  it("filters Active to queued and downloading and searches filename or URL", () => {
    const rows = [download(), download({ id: "2", status: "queued", fileName: "photo.png" }), download({ id: "3", status: "failed" }), download({ id: "4", status: "completed" }), download({ id: "5", status: "paused" })];
    expect(filterDownloads(rows, "active", "").map((item) => item.id)).toEqual(["download-1", "2"]);
    expect(filterDownloads(rows, "active", " PHOTO ").map((item) => item.id)).toEqual(["2"]);
    expect(filterDownloads(rows, "failed", "EXAMPLE.COM").map((item) => item.id)).toEqual(["3"]);
    expect(filterDownloads(rows, "all", "missing")).toEqual([]);
  });

  it("marks completed unknown-size and zero-byte files as complete", () => {
    expect(progressPercent(download({ totalBytes: null }))).toBeNull();
    expect(progressPercent(download({ status: "completed", totalBytes: null }))).toBe(100);
    expect(progressPercent(download({ status: "completed", totalBytes: 0, downloadedBytes: 0 }))).toBe(100);
    expect(progressPercent(download({ downloadedBytes: 5000 }))).toBe(100);
  });

  it("formats empty, large and unknown transfer information", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1_073_741_824)).toBe("1 GB");
    expect(formatEta(null)).toBe("—");
    expect(formatEta(0)).toBe("< 1 sec");
    expect(formatEta(65)).toBe("1 min 5 sec");
    expect(formatEta(3661)).toBe("1 hr 1 min");
  });
});

describe("form validation", () => {
  it.each(["", "example.com/file", "ftp://example.com/file", "https://user:secret@example.com/file"])("rejects invalid or unsupported URL %s", (url) => {
    expect(validateUrl(url)).toBeTruthy();
  });
  it.each(["http://localhost:8765/file", " https://example.com/file.zip "])("accepts HTTP(S) URL %s", (url) => {
    expect(validateUrl(url)).toBeUndefined();
  });
  it.each(["../file.zip", "bad?.zip", "CON.txt", "CONIN$.txt", "LPT¹.txt", "trailing.", "trailing ", "\u0000name", "é".repeat(111)])("rejects invalid Windows filename %s", (name) => {
    expect(validateFileName(name)).toBeTruthy();
  });
  it.each(["", "archive (1).zip", "report-final.pdf", "日本語.txt"])("accepts optional or valid filename %s", (name) => {
    expect(validateFileName(name)).toBeUndefined();
  });
});
