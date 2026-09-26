import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "../App";
import { api } from "../api";
import { useSnapshot } from "../useSnapshot";
import { download, snapshot } from "./fixtures";

vi.mock("../useSnapshot");
const refresh = vi.fn();
const completed = download({ id: "complete", fileName: "finished.zip", status: "completed", totalBytes: null });

beforeEach(() => {
  vi.mocked(useSnapshot).mockReturnValue({ snapshot: { ...snapshot, downloads: [download(), completed, download({ id: "queued", fileName: "waiting.pdf", status: "queued" }), download({ id: "failed", fileName: "broken.iso", status: "failed", error: "Server returned 404" })] }, error: null, loading: false, refresh });
});

describe("download workspace", () => {
  it("filters by status and searches the remaining rows", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: /^Active/ }));
    expect(screen.getByRole("button", { name: "View details for waiting.pdf" })).toBeVisible();
    expect(screen.getByRole("button", { name: "View details for archive.zip" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "View details for finished.zip" })).not.toBeInTheDocument();
    await user.type(screen.getByRole("textbox", { name: "Search downloads" }), "WAITING");
    expect(screen.getByRole("button", { name: "View details for waiting.pdf" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "View details for archive.zip" })).not.toBeInTheDocument();
  });

  it("shows completed unknown-size files as complete and opens/reveals by id", async () => {
    const user = userEvent.setup();
    const open = vi.spyOn(api, "openDownload").mockResolvedValue();
    render(<App />);
    await user.click(screen.getByRole("button", { name: "View details for finished.zip" }));
    const details = screen.getByRole("complementary", { name: "Download details" });
    expect(within(details).getByRole("progressbar")).toHaveAttribute("aria-valuenow", "100");
    await user.click(within(details).getByRole("button", { name: "Open file" }));
    expect(open).toHaveBeenLastCalledWith("complete", false);
    await user.click(within(details).getByRole("button", { name: "Show in folder" }));
    expect(open).toHaveBeenLastCalledWith("complete", true);
    await user.click(within(details).getByRole("button", { name: "Close details" }));
    expect(screen.getByRole("button", { name: "View details for finished.zip" })).toHaveFocus();
  });

  it("shows readable missing-file errors and failed transfer details", async () => {
    const user = userEvent.setup();
    vi.spyOn(api, "openDownload").mockRejectedValue("The downloaded file has moved or no longer exists.");
    render(<App />);
    await user.click(screen.getByRole("button", { name: "View details for finished.zip" }));
    await user.click(screen.getByRole("button", { name: "Open file" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("has moved or no longer exists");
    await user.click(screen.getByRole("button", { name: "View details for broken.iso" }));
    expect(screen.getByText("Server returned 404")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Open file" })).not.toBeInTheDocument();
  });

  it("refreshes immediately after adding and restores keyboard focus", async () => {
    const user = userEvent.setup();
    vi.spyOn(api, "addDownloads").mockResolvedValue([download()]);
    render(<App />);
    await user.click(screen.getByRole("button", { name: "Add download" }));
    await user.type(screen.getByLabelText(/Download URL/), "https://example.com/archive.zip");
    await user.click(screen.getByRole("button", { name: "Start download" }));
    expect(refresh).toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add download" })).toHaveFocus();
  });

  it("offers retry when initial loading fails without enabling Add", async () => {
    const user = userEvent.setup();
    vi.mocked(useSnapshot).mockReturnValue({ snapshot: null, error: "Engine unavailable", loading: false, refresh });
    render(<App />);
    expect(screen.getByRole("alert")).toHaveTextContent("Engine unavailable");
    expect(screen.getByRole("button", { name: "Add download" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(refresh).toHaveBeenCalled();
  });
});
