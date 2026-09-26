import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { api } from "../api";
import { useSnapshot } from "../useSnapshot";
import type { Snapshot } from "../types";
import { snapshot } from "./fixtures";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (value: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe("snapshot polling", () => {
  it("loads immediately, waits 500ms after completion, and never overlaps slow requests", async () => {
    vi.useFakeTimers();
    const first = deferred<Snapshot>();
    const get = vi.spyOn(api, "getSnapshot").mockReturnValueOnce(first.promise).mockResolvedValue(snapshot);
    const { result, unmount } = renderHook(useSnapshot);
    expect(get).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(true);
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(get).toHaveBeenCalledTimes(1);
    await act(async () => { first.resolve(snapshot); await first.promise; });
    expect(result.current.snapshot).toEqual(snapshot);
    await act(async () => { await vi.advanceTimersByTimeAsync(499); });
    expect(get).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(get).toHaveBeenCalledTimes(2);
    unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(get).toHaveBeenCalledTimes(2);
  });

  it("coalesces refreshes during a request and refreshes immediately after it settles", async () => {
    vi.useFakeTimers();
    const first = deferred<Snapshot>();
    const get = vi.spyOn(api, "getSnapshot").mockReturnValueOnce(first.promise).mockResolvedValue(snapshot);
    const { result } = renderHook(useSnapshot);
    act(() => { result.current.refresh(); result.current.refresh(); });
    expect(get).toHaveBeenCalledTimes(1);
    await act(async () => { first.resolve(snapshot); await first.promise; });
    expect(get).toHaveBeenCalledTimes(2);
    expect(result.current.snapshot).toEqual(snapshot);
  });

  it("preserves history on refresh failure and clears the error after retry", async () => {
    vi.useFakeTimers();
    vi.spyOn(api, "getSnapshot").mockResolvedValueOnce(snapshot).mockRejectedValueOnce("Database temporarily unavailable").mockResolvedValue(snapshot);
    const { result } = renderHook(useSnapshot);
    await act(async () => { await Promise.resolve(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
    expect(result.current.error).toBe("Database temporarily unavailable");
    expect(result.current.snapshot).toEqual(snapshot);
    await act(async () => { result.current.refresh(); });
    expect(result.current.error).toBeNull();
  });
});
