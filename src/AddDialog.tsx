import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { api, errorMessage } from "./api";
import { Icon } from "./Icon";
import { validateFileName, validateUrl } from "./lib";
import type { Download } from "./types";

interface Props {
  defaultDirectory: string;
  onClose: () => void;
  onAdded: (downloads: Download[]) => void;
}

export function AddDialog({ defaultDirectory, onClose, onAdded }: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const urlRef = useRef<HTMLInputElement>(null);
  const filenameRef = useRef<HTMLInputElement>(null);
  const pendingRef = useRef(false);
  const [url, setUrl] = useState("");
  const [fileName, setFileName] = useState("");
  const [destination, setDestination] = useState(defaultDirectory);
  const [errors, setErrors] = useState<{ url?: string; fileName?: string }>({});
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [choosing, setChoosing] = useState(false);

  useEffect(() => {
    const dialog = dialogRef.current!;
    dialog.showModal();
    urlRef.current?.focus();
    return () => { dialog.close(); };
  }, []);

  async function chooseDirectory() {
    setChoosing(true);
    setError(null);
    try {
      const selected = await api.chooseDirectory(destination || defaultDirectory || undefined);
      if (typeof selected === "string") setDestination(selected);
    } catch (cause) { setError(errorMessage(cause)); }
    finally { setChoosing(false); }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (pendingRef.current || choosing) return;
    const nextErrors = { url: validateUrl(url), fileName: validateFileName(fileName) };
    setErrors(nextErrors);
    setError(null);
    if (nextErrors.url || nextErrors.fileName) {
      (nextErrors.url ? urlRef : filenameRef).current?.focus();
      return;
    }
    pendingRef.current = true;
    setSubmitting(true);
    try {
      const added = await api.addDownloads([{
        url: url.trim(),
        fileName: fileName || null,
        destination: destination.trim() || null,
      }]);
      onAdded(added);
      onClose();
    } catch (cause) { setError(errorMessage(cause)); }
    finally { pendingRef.current = false; setSubmitting(false); }
  }

  return <dialog ref={dialogRef} className="add-dialog" aria-labelledby="add-title" aria-describedby="add-description"
    onCancel={(event) => { event.preventDefault(); if (!submitting && !choosing) onClose(); }}>
    <div className="dialog-heading">
      <span className="dialog-mark"><Icon name="download" size={24} /></span>
      <button type="button" className="icon-button" aria-label="Close dialog" disabled={submitting || choosing} onClick={onClose}><Icon name="x" /></button>
    </div>
    <h2 id="add-title">Add a download</h2>
    <p id="add-description" className="dialog-description">Paste a link. We’ll take it from here.</p>
    <form onSubmit={submit} noValidate>
      <fieldset disabled={submitting || choosing}>
        <div className="form-field">
          <label htmlFor="download-url">Download URL <span className="required-label">Required</span></label>
          <div className={`input-with-icon ${errors.url ? "invalid" : ""}`}><Icon name="link" size={18} />
            <input ref={urlRef} id="download-url" type="url" autoComplete="off" spellCheck={false} placeholder="https://example.com/file.zip"
              value={url} onChange={(event) => { setUrl(event.target.value); setErrors((current) => ({ ...current, url: undefined })); }}
              aria-invalid={!!errors.url} aria-describedby={errors.url ? "url-error" : undefined} />
          </div>
          {errors.url && <p id="url-error" className="field-error">{errors.url}</p>}
        </div>
        <div className="form-field">
          <label htmlFor="download-name">File name <span className="optional-label">Optional</span></label>
          <input ref={filenameRef} id="download-name" autoComplete="off" spellCheck={false} placeholder="Use the name from the URL" value={fileName}
            onChange={(event) => { setFileName(event.target.value); setErrors((current) => ({ ...current, fileName: undefined })); }}
            aria-invalid={!!errors.fileName} aria-describedby={errors.fileName ? "name-error" : "name-hint"} />
          {errors.fileName ? <p id="name-error" className="field-error">{errors.fileName}</p> : <p id="name-hint" className="field-hint">Existing files are kept. We’ll choose an available name.</p>}
        </div>
        <div className="form-field">
          <label htmlFor="download-destination">Save to</label>
          <div className="directory-input"><div className="input-with-icon"><Icon name="folder" size={18} /><input id="download-destination" value={destination} placeholder={defaultDirectory || "Your Downloads folder"}
            onChange={(event) => setDestination(event.target.value)} aria-describedby="destination-hint" /></div>
            <button type="button" className="button secondary" onClick={() => void chooseDirectory()}>Browse</button>
          </div>
          <p id="destination-hint" className="field-hint">{choosing ? "Choose a folder in the folder picker." : "Leave blank to use your default Downloads folder."}</p>
        </div>
      </fieldset>
      {error && <div className="inline-error" role="alert"><Icon name="alert" size={18} /><span>{error}</span></div>}
      <div className="dialog-footer"><button type="button" className="button secondary" disabled={submitting || choosing} onClick={onClose}>Cancel</button>
        <button type="submit" className="button primary" disabled={submitting || choosing}><Icon name={submitting ? "refresh" : "download"} size={18} />{submitting ? "Adding…" : "Start download"}</button>
      </div>
    </form>
  </dialog>;
}
