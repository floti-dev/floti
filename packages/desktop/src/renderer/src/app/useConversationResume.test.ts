import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { HistorySession, SessionRecord } from "@helm/core";
import { bridge, remoteBridge } from "./bridge.testkit";
import { useConversationResume } from "./useConversationResume";

vi.mock("./bridge", () => import("./bridge.testkit"));

const record = { id: 7, status: "running" } as unknown as SessionRecord;

afterEach(() => {
  bridge.reset();
  remoteBridge("h1").reset();
});

function setup() {
  const adopt = vi.fn();
  const { result } = renderHook(() =>
    useConversationResume({ grid: () => ({ cols: 120, rows: 40 }), adopt }),
  );
  return { result, adopt };
}

describe("a resume from History", () => {
  it("resumes a conversation on another computer through that host, as its agent, and places what it started", async () => {
    remoteBridge("h1").answer("history:resume", () => ({ session: record, history: {} as HistorySession }));
    const { result, adopt } = setup();
    let refused: string | null = "unset";
    await act(async () => {
      refused = await result.current.resumeOn("h1", "c1", "lead");
    });
    expect(refused).toBeNull();
    expect(remoteBridge("h1").invoked("history:resume")).toEqual([
      { sessionId: "c1", agent: "lead", cols: 120, rows: 40 },
    ]);
    expect(bridge.invoked("history:resume")).toEqual([]);
    expect(adopt).toHaveBeenCalledWith("h1", record);
  });

  it("shows the session already running a conversation, which the host names in place of starting another, as one that came back", async () => {
    const running = { id: 3, status: "running" } as unknown as SessionRecord;
    bridge.answer("history:resume", () => ({ session: running, history: {} as HistorySession, alreadyRunning: true }));
    const { result, adopt } = setup();
    let refused: string | null = "unset";
    await act(async () => {
      refused = await result.current.resumeOn("local", "c1", undefined);
    });
    expect(refused).toBeNull();
    expect(adopt).toHaveBeenCalledTimes(1);
    expect(adopt).toHaveBeenCalledWith("local", running);
  });

  it("says a refusal with the host's sentence and places nothing", async () => {
    bridge.answer("history:resume", () => {
      throw new Error("Its folder is gone.");
    });
    const { result, adopt } = setup();
    let refused: string | null = null;
    await act(async () => {
      refused = await result.current.resumeOn("local", "c1", undefined);
    });
    expect(refused).toBe("Its folder is gone.");
    expect(bridge.invoked("history:resume")).toEqual([{ sessionId: "c1", cols: 120, rows: 40 }]);
    expect(adopt).not.toHaveBeenCalled();
  });

  it("starts one at a time: a second press while one is in flight starts nothing, and the next after it ends does", async () => {
    let settle: () => void = () => undefined;
    const { result } = setup();
    const task = vi.fn(() => new Promise<void>((resolve) => (settle = resolve)));
    let first: Promise<void> | null = null;
    let second: Promise<void> | null = Promise.resolve();
    act(() => {
      first = result.current.exclusive("c1", task);
      second = result.current.exclusive("c1", task);
    });
    expect(first).not.toBeNull();
    expect(second).toBeNull();
    expect(task).toHaveBeenCalledTimes(1);
    expect(result.current.resuming).toBe("c1");
    await act(async () => {
      settle();
      await first;
    });
    expect(result.current.resuming).toBeNull();
    const next = vi.fn(async () => undefined);
    await act(async () => {
      await result.current.exclusive("c2", next);
    });
    expect(next).toHaveBeenCalledTimes(1);
  });
});
