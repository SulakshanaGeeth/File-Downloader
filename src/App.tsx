import { useEffect, useRef, useState } from "react";
import { AddDialog } from "./AddDialog";
import { api, errorMessage } from "./api";
import { Icon, fileKind } from "./Icon";
import type { IconName } from "./Icon";
import { filterDownloads, formatBytes, formatEta, isActive, progressPercent, sourceHost } from "./lib";
import type { Download, DownloadStatus, Filter } from "./types";
import { useSnapshot } from "./useSnapshot";

const statuses: Record<DownloadStatus, string> = { queued: "Queued", downloading: "Downloading", completed: "Completed", failed: "Failed", paused: "Paused", cancelled: "Cancelled" };
const navigation: { id: Filter; label: string; icon: IconName; description: string }[] = [
  { id: "all", label: "All downloads", icon: "grid", description: "Everything you need, in one place." },
  { id: "active", label: "Active", icon: "activity", description: "Your downloads, moving along." },
  { id: "completed", label: "Completed", icon: "check", description: "Ready when you are." },
  { id: "failed", label: "Failed", icon: "alert", description: "Downloads that need your attention." },
];

function StatusBadge({ status }: { status: DownloadStatus }) {
  return <span className={`status-badge status-${status}`}><span className="status-dot" />{statuses[status]}</span>;
}

function FileIcon({ name, large = false }: { name: string; large?: boolean }) {
  const kind = fileKind(name);
  return <span className={`file-icon file-${kind.color} ${large ? "file-icon-large" : ""}`}><Icon name={kind.icon} size={large ? 30 : 21} /></span>;
}

function Progress({ download, detail = false }: { download: Download; detail?: boolean }) {
  const percent = progressPercent(download);
  const indeterminate = percent === null && download.status === "downloading";
  return <div className={`transfer-progress ${detail ? "detail-progress" : ""}`}>
    <div className="progress-label"><span>{formatBytes(download.downloadedBytes)}{download.totalBytes !== null && <> <span className="muted">/ {formatBytes(download.totalBytes)}</span></>}</span>
      {detail && <strong>{percent === null ? "Size unknown" : `${Math.floor(percent)}%`}</strong>}
    </div>
    <div className={`progress-track ${indeterminate ? "indeterminate" : ""} progress-${download.status}`} role="progressbar"
      aria-label={`Progress for ${download.fileName}`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent ?? undefined}
      aria-valuetext={percent === null ? `${formatBytes(download.downloadedBytes)} downloaded; total size unknown` : `${Math.floor(percent)}%`}>
      <span style={{ width: indeterminate ? undefined : `${percent ?? 0}%` }} />
    </div>
  </div>;
}

function DownloadDetails({ download, onOpen, opening, onClose }: { download: Download | null; onOpen: (download: Download, reveal: boolean) => void; opening: string | null; onClose: () => void }) {
  if (!download) return <aside className="details-panel empty-details" aria-label="Download details"><div className="details-label">FILE DETAILS</div><div className="details-placeholder"><span className="details-placeholder-icon"><Icon name="file" size={30} /></span><h3>A closer look</h3><p>Select a download to see its details and file location.</p></div></aside>;
  const isDownloading = download.status === "downloading";
  return <aside className="details-panel" aria-label="Download details">
    <div className="details-heading"><span className="details-label">FILE DETAILS</span><button className="icon-button" aria-label="Close details" onClick={onClose}><Icon name="x" size={17} /></button></div>
    <div className="details-file"><FileIcon name={download.fileName} large /><h2>{download.fileName}</h2><span className="file-type">{fileKind(download.fileName).label}</span><StatusBadge status={download.status} /></div>
    <Progress download={download} detail />
    {download.error && <div className="detail-error" role="note"><Icon name="alert" size={17} /><div><strong>Download failed</strong><p>{download.error}</p></div></div>}
    <dl className="detail-fields">
      <div><dt><Icon name="link" size={15} />Source URL</dt><dd className="selectable">{download.url}</dd></div>
      <div><dt><Icon name="folder" size={15} />Saved location</dt><dd className="selectable">{download.destination.replace(/[\\/]$/, "")}\{download.fileName}</dd></div>
      <div><dt><Icon name="clock" size={15} />Added</dt><dd>{new Date(download.createdAt).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}</dd></div>
    </dl>
    <div className="transfer-facts"><div><span>Speed</span><strong>{isDownloading ? `${formatBytes(download.speedBps)}/s` : "—"}</strong></div><div><span>Remaining</span><strong>{isDownloading ? formatEta(download.etaSeconds) : "—"}</strong></div><div><span>Connections</span><strong>{download.connections}</strong></div><div><span>File size</span><strong>{download.totalBytes === null ? download.status === "completed" ? formatBytes(download.downloadedBytes) : "Unknown" : formatBytes(download.totalBytes)}</strong></div></div>
    {download.status === "completed" && <div className="details-actions"><button className="button primary" onClick={() => onOpen(download, false)} disabled={opening !== null}><Icon name="arrow" size={17} />{opening === `${download.id}:open` ? "Opening…" : "Open file"}</button><button className="button secondary" onClick={() => onOpen(download, true)} disabled={opening !== null}><Icon name="folder" size={17} />{opening === `${download.id}:reveal` ? "Opening folder…" : "Show in folder"}</button></div>}
  </aside>;
}

export default function App() {
  const { snapshot, error: snapshotError, loading, refresh } = useSnapshot();
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [opening, setOpening] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const openingRef = useRef(false);
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const downloadsRef = useRef<HTMLDivElement>(null);
  const downloads = snapshot?.downloads ?? [];
  const visible = filterDownloads(downloads, filter, query);
  const selected = visible.find((download) => download.id === selectedId) ?? null;
  const active = downloads.filter(isActive);
  const running = downloads.filter((download) => download.status === "downloading");
  const completed = downloads.filter((download) => download.status === "completed");
  const failed = downloads.filter((download) => download.status === "failed");
  const speed = running.reduce((total, download) => total + download.speedBps, 0);
  const transferred = downloads.reduce((total, download) => total + download.downloadedBytes, 0);
  const counts = { all: downloads.length, active: active.length, completed: completed.length, failed: failed.length };
  const currentNavigation = navigation.find((item) => item.id === filter)!;

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (showAdd || event.altKey || event.repeat) return;
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") { event.preventDefault(); searchRef.current?.focus(); }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "n" && snapshot) { event.preventDefault(); setShowAdd(true); }
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [showAdd, snapshot]);

  async function openDownload(download: Download, reveal: boolean) {
    if (openingRef.current) return;
    openingRef.current = true;
    setOpening(`${download.id}:${reveal ? "reveal" : "open"}`);
    setActionError(null);
    try { await api.openDownload(download.id, reveal); }
    catch (cause) { setActionError(errorMessage(cause)); }
    finally { openingRef.current = false; setOpening(null); }
  }

  return <div className="app-shell">
    <aside className="sidebar">
      <div className="brand"><span className="brand-symbol"><Icon name="download" size={23} /></span><span className="brand-name">flow<span className="brand-dot">.</span></span></div>
      <div className="sidebar-section-label">LIBRARY</div>
      <nav aria-label="Download filters">{navigation.map((item) => <button key={item.id} className={`nav-item ${filter === item.id ? "active" : ""}`} aria-current={filter === item.id ? "page" : undefined}
        onClick={() => { setFilter(item.id); setSelectedId(null); }}><Icon name={item.icon} size={19} /><span>{item.label}</span><span className="nav-count">{counts[item.id]}</span></button>)}</nav>
      <div className="sidebar-bottom"><div className={`engine-state ${snapshotError ? "engine-error" : ""}`}><span />{snapshotError ? "Connection interrupted" : loading ? "Connecting…" : running.length > 0 ? "Downloads in progress" : "Ready to download"}</div><p>Less waiting. More doing.</p><span className="version">FLOW · v0.1.0</span></div>
    </aside>
    <main className="workspace">
      <header className="page-header"><div><div className="eyebrow">YOUR WORKSPACE</div><h1>{currentNavigation.label}</h1><p>{currentNavigation.description}</p></div><button ref={addButtonRef} className="button primary add-button" disabled={!snapshot} onClick={() => setShowAdd(true)} title="Add download (Ctrl+N)"><Icon name="plus" size={19} />Add download</button></header>
      <section className="summary-strip" aria-label="Download summary"><div className="summary-item"><span className="summary-icon blue"><Icon name="activity" size={21} /></span><div><span className="summary-label">Downloading</span><strong>{running.length}<span className="summary-context">{active.length - running.length > 0 ? `${active.length - running.length} queued` : "files"}</span></strong></div></div><div className="summary-item"><span className="summary-icon green"><Icon name="check" size={21} /></span><div><span className="summary-label">Completed</span><strong>{completed.length}<span className="summary-context">files</span></strong></div></div><div className="summary-item"><span className="summary-icon purple"><Icon name="download" size={21} /></span><div><span className="summary-label">Total transferred</span><strong className="bytes-summary">{formatBytes(transferred)}</strong></div></div><div className="summary-item speed-summary"><span className="summary-icon neutral"><Icon name="speed" size={21} /></span><div><span className="summary-label">Current speed</span><strong className="bytes-summary">{formatBytes(speed)}<span className="speed-unit">/s</span></strong></div></div></section>
      {snapshotError && <div className="banner error-banner" role="alert"><Icon name="alert" size={19} /><div><strong>{snapshot ? "Couldn’t refresh downloads" : "Couldn’t connect to the download engine"}</strong><span>{snapshotError}</span></div><button className="button secondary compact" onClick={refresh}><Icon name="refresh" size={15} />Retry</button></div>}
      {actionError && <div className="banner error-banner" role="alert"><Icon name="alert" size={19} /><div><strong>Couldn’t open this file</strong><span>{actionError}</span></div><button className="icon-button" aria-label="Dismiss error" onClick={() => setActionError(null)}><Icon name="x" size={18} /></button></div>}
      <section className="library-panel" aria-label="Downloads">
        <div className="library-toolbar"><div className="library-title">Files <span>{visible.length}</span></div><div className="search-box"><Icon name="search" size={17} /><input ref={searchRef} aria-label="Search downloads" placeholder="Search files or URLs…" value={query} onChange={(event) => setQuery(event.target.value)} />{query ? <button className="icon-button" aria-label="Clear search" onClick={() => { setQuery(""); searchRef.current?.focus(); }}><Icon name="x" size={15} /></button> : <kbd>Ctrl K</kbd>}</div></div>
        <div className="library-body"><div className="downloads-area" ref={downloadsRef}>
          {loading ? <div className="empty-state" role="status"><span className="loading-spinner" /><h2>Getting things ready</h2><p>Loading your download history…</p></div> : !snapshot ? <div className="empty-state"><span className="empty-icon"><Icon name="alert" size={31} /></span><h2>Let’s reconnect</h2><p>Your downloads will appear here when the engine is available.</p><button className="button secondary" onClick={refresh}><Icon name="refresh" size={17} />Try again</button></div> : visible.length === 0 ? <div className="empty-state"><span className="empty-icon"><Icon name={query ? "search" : filter === "completed" ? "check" : filter === "failed" ? "check" : "download"} size={32} /></span><span className="empty-decoration" /><h2>{query ? "No matching downloads" : filter === "all" ? "Make room for what’s next" : filter === "active" ? "You’re all caught up" : filter === "completed" ? "Good things are on the way" : "All clear here"}</h2><p>{query ? "Try a different filename or a part of the URL." : filter === "all" ? "Your next file is just a link away. Add a download to get started." : filter === "active" ? "There are no downloads in progress. Add a link whenever you’re ready." : filter === "completed" ? "Finished downloads will appear here, ready to open." : "No failed downloads. Everything is looking good."}</p>{query ? <button className="button secondary" onClick={() => setQuery("")}>Clear search</button> : (filter === "all" || filter === "active" || filter === "completed") && <button className="button primary" onClick={() => setShowAdd(true)}><Icon name="plus" size={17} />Add your {downloads.length === 0 ? "first " : "next "}download</button>}<span className="empty-footnote">{query || filter === "failed" ? "" : "HTTP & HTTPS links supported"}</span></div> : <div className="table-scroll"><table className="download-table"><thead><tr><th>File name</th><th>Status</th><th>Progress</th><th>Speed</th><th>Remaining</th></tr></thead><tbody>{visible.map((download) => <tr key={download.id} className={selectedId === download.id ? "selected" : ""} onClick={() => setSelectedId(download.id)}><td><button className="file-select" aria-label={`View details for ${download.fileName}`} aria-pressed={selectedId === download.id} onClick={() => setSelectedId(download.id)}><FileIcon name={download.fileName} /><span className="file-text"><strong title={download.fileName}>{download.fileName}</strong><span>{sourceHost(download.url)}</span></span></button></td><td><StatusBadge status={download.status} /></td><td><Progress download={download} /></td><td className="numeric-cell">{download.status === "downloading" ? `${formatBytes(download.speedBps)}/s` : "—"}</td><td className="numeric-cell">{download.status === "downloading" ? formatEta(download.etaSeconds) : download.status === "completed" ? <span className="done-label">Done</span> : "—"}</td></tr>)}</tbody></table></div>}
        </div><DownloadDetails download={selected} onOpen={(download, reveal) => void openDownload(download, reveal)} opening={opening} onClose={() => { downloadsRef.current?.querySelector<HTMLButtonElement>('button[aria-pressed="true"]')?.focus(); setSelectedId(null); }} /></div>
        <footer className="library-footer"><span>{visible.length} {visible.length === 1 ? "download" : "downloads"}{query || filter !== "all" ? ` of ${downloads.length}` : ""}</span><span><span className={`live-dot ${snapshotError ? "disconnected" : ""}`} />{snapshotError ? "Waiting to reconnect" : loading ? "Connecting" : "Up to date"}</span></footer>
      </section>
      <div className="workspace-footer"><span>YOUR FILES. YOUR FLOW.</span><span><Icon name="folder" size={14} /><span title={snapshot?.settings.downloadDir}>{snapshot?.settings.downloadDir ?? "Downloads folder"}</span></span></div>
    </main>
    <div className="sr-only" role="status" aria-live="polite">{announcement}</div>
    {showAdd && <AddDialog defaultDirectory={snapshot?.settings.downloadDir ?? ""} onClose={() => { setShowAdd(false); addButtonRef.current?.focus(); }} onAdded={(added) => { refresh(); setFilter("all"); setQuery(""); setSelectedId(added[0]?.id ?? null); setAnnouncement(`${added[0]?.fileName ?? "Download"} added to your downloads.`); }} />}
  </div>;
}
