import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RestorableSession } from "@helm/core/types";
import { useStoppedGroup, type StoppedGroupDeps } from "./useStoppedGroup";

const lost = (id: number, patch: Partial<RestorableSession> = {}): RestorableSession => ({
  id,
  name: `session ${String(id)}`,
  cwd: "/code/ledgerline",
  branch: null,
  profile: null,
  profileGone: false,
  lastAt: null,
  blocked: null,
  conversationId: `Conv-${String(id)}`,
  ...patch,
});

/** One Floti never learned the conversation of. */
const unknown = (id: number, patch: Partial<RestorableSession> = {}): RestorableSession => {
  const session = lost(id, patch);
  delete session.conversationId;
  return session;
};

function setup(patch: Partial<StoppedGroupDeps> = {}) {
  const resume = vi.fn<StoppedGroupDeps["resume"]>(async () => null);
  const raiseUndo = vi.fn<StoppedGroupDeps["raiseUndo"]>();
  const initial: StoppedGroupDeps = { running: new Set(), resume, raiseUndo, ...patch };
  const hook = renderHook((deps: StoppedGroupDeps) => useStoppedGroup(deps), { initialProps: initial });
  return { hook, resume, raiseUndo, initial };
}

const ids = (group: { sessions: readonly RestorableSession[] } | null) =>
  group === null ? null : group.sessions.map((session) => session.id);

describe("the Stopped group", () => {
  it("shows nothing until a stop's sessions are kept, and nothing for an empty keep", () => {
    const { hook } = setup();
    expect(hook.result.current.shown).toBeNull();
    act(() => hook.result.current.keep([], null));
    expect(hook.result.current.shown).toBeNull();
    act(() => hook.result.current.keep([lost(1), lost(2)], null));
    expect(hook.result.current.shown).toEqual({ at: null, sessions: [lost(1), lost(2)] });
  });

  it("is headed by when the host found its sessions lost", () => {
    const { hook } = setup();
    act(() => hook.result.current.keep([lost(1)], 1_761_560_100_000));
    expect(hook.result.current.shown).toEqual({ at: 1_761_560_100_000, sessions: [lost(1)] });
  });

  it("drops a session once its conversation runs again, and goes when all are back", () => {
    const { hook, initial } = setup();
    act(() => hook.result.current.keep([lost(1), unknown(2)], null));
    hook.rerender({ ...initial, running: new Set(["conv-1"]) });
    expect(ids(hook.result.current.shown)).toEqual([2]);
    act(() => hook.result.current.keep([lost(1)], null));
    expect(ids(hook.result.current.shown)).toEqual([2]);
  });

  it("does not take back a session resumed elsewhere when it exits", () => {
    const { hook, initial } = setup();
    act(() => hook.result.current.keep([lost(1), lost(2)], null));
    hook.rerender({ ...initial, running: new Set(["conv-1"]) });
    hook.rerender({ ...initial, running: new Set() });
    expect(ids(hook.result.current.shown)).toEqual([2]);
  });

  it("adds a second stop's sessions to the first's, headed by the later stop", () => {
    const { hook } = setup();
    act(() => hook.result.current.keep([lost(1), lost(2)], 100));
    act(() => hook.result.current.keep([lost(2, { title: "again" }), lost(3)], 200));
    expect(ids(hook.result.current.shown)).toEqual([1, 2, 3]);
    expect(hook.result.current.shown?.at).toBe(200);
    expect(hook.result.current.shown?.sessions[1]?.title).toBe("again");
  });

  it("brings back what was dismissed beside a stop kept since, on Undo", async () => {
    const { hook, raiseUndo } = setup();
    act(() => hook.result.current.keep([lost(1)], null));
    act(() => hook.result.current.dismiss());
    act(() => hook.result.current.keep([lost(2)], null));
    const undo = raiseUndo.mock.calls[0]![2];
    await act(async () => {
      await undo();
    });
    expect(ids(hook.result.current.shown)).toEqual([2, 1]);
  });

  it("resumes each by its conversation, as its agent unless its agent has a problem", async () => {
    const { hook, resume } = setup();
    const sessions = [lost(1, { agent: "lead" }), lost(2, { agent: "worker", agentProblem: "worker.md is gone." })];
    act(() => hook.result.current.keep(sessions, null));
    let refused: string | null = "unset";
    await act(async () => {
      refused = await hook.result.current.resume(sessions);
    });
    expect(resume.mock.calls).toEqual([
      ["Conv-1", "lead"],
      ["Conv-2", undefined],
    ]);
    expect(refused).toBeNull();
    expect(hook.result.current.shown).toBeNull();
  });

  it("keeps a refused session and one with no conversation, and says the last refusal", async () => {
    const resume = vi.fn<StoppedGroupDeps["resume"]>(async (id) => (id === "Conv-2" ? "Its folder is gone." : null));
    const { hook } = setup({ resume });
    const sessions = [lost(1), lost(2), unknown(3, { title: "Export queue" })];
    act(() => hook.result.current.keep(sessions, null));
    let refused: string | null = null;
    await act(async () => {
      refused = await hook.result.current.resume(sessions.slice(0, 2));
    });
    expect(refused).toBe("Its folder is gone.");
    expect(ids(hook.result.current.shown)).toEqual([2, 3]);
    await act(async () => {
      refused = await hook.result.current.resume([sessions[2]!]);
    });
    expect(refused).toBe("Floti never learned which conversation Export queue was, so it cannot be resumed.");
    expect(resume).toHaveBeenCalledTimes(2);
    expect(ids(hook.result.current.shown)).toEqual([2, 3]);
  });

  it("is dismissed with an Undo that brings back what it showed", async () => {
    const { hook, raiseUndo } = setup();
    act(() => hook.result.current.keep([lost(1), lost(2)], null));
    act(() => hook.result.current.dismiss());
    expect(hook.result.current.shown).toBeNull();
    expect(raiseUndo).toHaveBeenCalledTimes(1);
    const [title, detail, undo] = raiseUndo.mock.calls[0]!;
    expect(title).toBe("Stopped sessions dismissed");
    expect(detail).toBe("2 sessions the stop took are no longer at the top of History.");
    await act(async () => {
      expect(await undo()).toBeNull();
    });
    expect(ids(hook.result.current.shown)).toEqual([1, 2]);
  });

  it("raises nothing when there is nothing to dismiss", () => {
    const { hook, raiseUndo } = setup();
    act(() => hook.result.current.dismiss());
    expect(raiseUndo).not.toHaveBeenCalled();
  });
});
