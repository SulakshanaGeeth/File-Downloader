import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { AddRequest, Download, Snapshot } from "./types";

export const api = {
  getSnapshot: () => invoke<Snapshot>("get_snapshot"),
  addDownloads: (requests: AddRequest[]) => invoke<Download[]>("add_downloads", { requests }),
  openDownload: (id: string, reveal: boolean) => invoke<void>("open_download", { id, reveal }),
  chooseDirectory: (defaultPath?: string) => open({ directory: true, multiple: false, defaultPath, title: "Choose download folder" }),
};

export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong. Please try again.";
}
