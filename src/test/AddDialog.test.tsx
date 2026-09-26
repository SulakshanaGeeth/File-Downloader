import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AddDialog } from "../AddDialog";
import { api } from "../api";
import { download, snapshot } from "./fixtures";

describe("add download dialog", () => {
  it("focuses the URL, rejects invalid inputs, and focuses the first error", async () => {
    const user = userEvent.setup();
    const add = vi.spyOn(api, "addDownloads");
    render(<AddDialog defaultDirectory={snapshot.settings.downloadDir} onClose={vi.fn()} onAdded={vi.fn()} />);
    const url = screen.getByLabelText(/Download URL/);
    expect(url).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Start download" }));
    expect(screen.getByText("Enter a download URL.")).toBeVisible();
    expect(url).toHaveFocus();
    await user.type(url, "https://example.com/file");
    await user.type(screen.getByLabelText(/File name/), "CON.txt");
    await user.click(screen.getByRole("button", { name: "Start download" }));
    expect(screen.getByText(/reserved by Windows/)).toBeVisible();
    expect(screen.getByLabelText(/File name/)).toHaveFocus();
    expect(add).not.toHaveBeenCalled();
  });

  it("uses folder picker and submits camelCase data once even on repeated submission", async () => {
    const user = userEvent.setup();
    let finish!: (downloads: ReturnType<typeof download>[]) => void;
    const add = vi.spyOn(api, "addDownloads").mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    vi.spyOn(api, "chooseDirectory").mockResolvedValue("D:\\Files");
    const onAdded = vi.fn();
    const onClose = vi.fn();
    render(<AddDialog defaultDirectory={snapshot.settings.downloadDir} onClose={onClose} onAdded={onAdded} />);
    await user.type(screen.getByLabelText(/Download URL/), "https://example.com/archive.zip");
    await user.click(screen.getByRole("button", { name: "Browse" }));
    expect(screen.getByLabelText("Save to")).toHaveValue("D:\\Files");
    const form = screen.getByRole("button", { name: "Start download" }).closest("form")!;
    fireEvent.submit(form);
    fireEvent.submit(form);
    expect(add).toHaveBeenCalledTimes(1);
    expect(add).toHaveBeenCalledWith([{ url: "https://example.com/archive.zip", fileName: null, destination: "D:\\Files" }]);
    expect(screen.getByRole("button", { name: "Adding…" })).toBeDisabled();
    fireEvent(screen.getByRole("dialog"), new Event("cancel", { cancelable: true }));
    expect(onClose).not.toHaveBeenCalled();
    finish([download()]);
    await waitFor(() => expect(onAdded).toHaveBeenCalledWith([download()]));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps entered data and shows backend errors so submission can be retried", async () => {
    const user = userEvent.setup();
    vi.spyOn(api, "addDownloads").mockRejectedValue("Destination is not a directory: D:\\file.txt");
    render(<AddDialog defaultDirectory="D:\\file.txt" onClose={vi.fn()} onAdded={vi.fn()} />);
    await user.type(screen.getByLabelText(/Download URL/), "https://example.com/file");
    await user.click(screen.getByRole("button", { name: "Start download" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Destination is not a directory");
    expect(screen.getByLabelText(/Download URL/)).toHaveValue("https://example.com/file");
    expect(screen.getByRole("button", { name: "Start download" })).toBeEnabled();
  });

  it("dismisses with the dialog's native Escape cancellation event", () => {
    const onClose = vi.fn();
    render(<AddDialog defaultDirectory="" onClose={onClose} onAdded={vi.fn()} />);
    fireEvent(screen.getByRole("dialog"), new Event("cancel", { cancelable: true }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
