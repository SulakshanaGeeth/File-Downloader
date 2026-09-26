import type { CSSProperties } from "react";

export type IconName = "download" | "grid" | "activity" | "check" | "alert" | "search" | "plus" | "folder" | "arrow" | "file" | "archive" | "image" | "video" | "music" | "code" | "x" | "link" | "chevron" | "clock" | "speed" | "refresh";
const paths: Record<IconName, React.ReactNode> = {
  download: <><path d="M12 3v12m-5-5 5 5 5-5" /><path d="M5 15v5h14v-5" /></>,
  grid: <><rect x="4" y="4" width="6" height="6" rx="1.5" /><rect x="14" y="4" width="6" height="6" rx="1.5" /><rect x="4" y="14" width="6" height="6" rx="1.5" /><rect x="14" y="14" width="6" height="6" rx="1.5" /></>,
  activity: <path d="M3 12h4l3-7 4 14 3-7h4" />,
  check: <><circle cx="12" cy="12" r="8.5" /><path d="m8 12 2.5 2.5 5.5-5.5" /></>,
  alert: <><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5v5m0 3.5v.1" /></>,
  search: <><circle cx="10.5" cy="10.5" r="6.5" /><path d="m16 16 4 4" /></>,
  plus: <path d="M12 5v14M5 12h14" />,
  folder: <path d="M3 7a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />,
  arrow: <><path d="M8 5h11v11M19 5 5 19" /><path d="M12 5H6a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-6" /></>,
  file: <><path d="M13 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V10Z" /><path d="M13 3v7h7M8 14h8M8 17h5" /></>,
  archive: <><rect x="4" y="3" width="16" height="18" rx="2" /><path d="M10 3v3h4v3h-4v3h4v3h-4v3h4" /></>,
  image: <><rect x="3" y="3" width="18" height="18" rx="2" /><circle cx="8" cy="8" r="1" /><path d="m3 17 5-5 4 4 4-6 5 7" /></>,
  video: <><rect x="3" y="3" width="18" height="18" rx="2" /><path d="m10 8 6 4-6 4Z" /></>,
  music: <><path d="M9 18V5l11-2v13M9 9l11-2" /><ellipse cx="6" cy="18" rx="3" ry="2" /><ellipse cx="17" cy="16" rx="3" ry="2" /></>,
  code: <><path d="m8 6-6 6 6 6m8-12 6 6-6 6m-3-15-2 18" /></>,
  x: <path d="m6 6 12 12M6 18 18 6" />,
  link: <><path d="m10 7 2-2a5 5 0 0 1 7 7l-2 2M7 10l-2 2a5 5 0 0 0 7 7l2-2m-6-1 8-8" /></>,
  chevron: <path d="m9 5 7 7-7 7" />,
  clock: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></>,
  speed: <><path d="M4 18a9 9 0 1 1 16 0M12 12l5-5" /><circle cx="12" cy="12" r="1.5" /></>,
  refresh: <><path d="M20 4v5h-5M4 20v-5h5" /><path d="M19 8A8 8 0 0 0 5 6M5 16a8 8 0 0 0 14 2" /></>,
};
export function Icon({ name, size = 20, style }: { name: IconName; size?: number; style?: CSSProperties }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>{paths[name]}</svg>;
}

export function fileKind(name: string): { icon: IconName; color: string; label: string } {
  const extension = name.split(".").pop()?.toLowerCase() ?? "";
  if (["zip", "rar", "7z", "tar", "gz", "iso"].includes(extension)) return { icon: "archive", color: "amber", label: "Archive" };
  if (["png", "jpg", "jpeg", "webp", "gif", "svg", "avif"].includes(extension)) return { icon: "image", color: "purple", label: "Image" };
  if (["mp4", "mov", "mkv", "webm", "avi"].includes(extension)) return { icon: "video", color: "pink", label: "Video" };
  if (["mp3", "wav", "flac", "ogg", "m4a"].includes(extension)) return { icon: "music", color: "pink", label: "Audio" };
  if (["exe", "msi", "dmg", "appimage"].includes(extension)) return { icon: "grid", color: "blue", label: "Application" };
  if (["js", "ts", "json", "html", "css", "rs", "py"].includes(extension)) return { icon: "code", color: "blue", label: "Code" };
  return { icon: "file", color: "blue", label: extension ? `${extension.toUpperCase()} file` : "File" };
}
