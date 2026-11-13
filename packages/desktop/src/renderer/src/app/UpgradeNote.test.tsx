import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AppSettings } from "@helm/core/types";
import { UpgradeNote, upgradeNoteDue, type UpgradeNoteProps } from "./UpgradeNote";

/**
 * The upgrade note as the window draws it: due where the database left it
 * unseen, and closed for good by any way out of it.
 */

const SEEN = "2026-11-13T09:00:00.000Z";
const note = () => screen.queryByRole("dialog", { name: "Helm is now Floti" });

function renderNote(patch: Partial<UpgradeNoteProps> = {}) {
  const writes: Array<Partial<AppSettings>> = [];
  const props: UpgradeNoteProps = {
    settings: { upgradeNoteSeenAt: null },
    setupNeeded: false,
    writeSettings: (next) => writes.push(next),
    onWhatsNew: vi.fn(),
    ...patch,
  };
  const view = render(<UpgradeNote {...props} />);
  return { ...view, props, writes };
}

describe("upgradeNoteDue", () => {
  it("is due where the database left it unseen and setup is not on screen", () => {
    expect(upgradeNoteDue({ upgradeNoteSeenAt: null }, false)).toBe(true);
  });

  it("is not due again once it has been closed", () => {
    expect(upgradeNoteDue({ upgradeNoteSeenAt: SEEN }, false)).toBe(false);
  });

  it("waits while setup is on screen", () => {
    expect(upgradeNoteDue({ upgradeNoteSeenAt: null }, true)).toBe(false);
  });

  it("is not due before the settings are read", () => {
    expect(upgradeNoteDue(null, false)).toBe(false);
  });
});

describe("UpgradeNote", () => {
  it("draws the note when it is due", () => {
    renderNote();
    expect(note()).not.toBeNull();
  });

  it("draws nothing when it was seen, while setup is on screen, or before the settings are read", () => {
    const { rerender, props } = renderNote({ settings: { upgradeNoteSeenAt: SEEN } });
    expect(note()).toBeNull();
    rerender(<UpgradeNote {...props} settings={{ upgradeNoteSeenAt: null }} setupNeeded />);
    expect(note()).toBeNull();
    rerender(<UpgradeNote {...props} settings={null} setupNeeded={false} />);
    expect(note()).toBeNull();
  });

  it("writes upgradeNoteSeenAt on Got it and is gone at once", async () => {
    const { writes, props } = renderNote();
    await userEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(note()).toBeNull();
    expect(writes).toHaveLength(1);
    expect(Number.isNaN(Date.parse(writes[0]?.upgradeNoteSeenAt ?? ""))).toBe(false);
    expect(props.onWhatsNew).not.toHaveBeenCalled();
  });

  it("writes upgradeNoteSeenAt on Escape", async () => {
    const { writes } = renderNote();
    await userEvent.keyboard("{Escape}");
    expect(note()).toBeNull();
    expect(writes.map((write) => Object.keys(write))).toEqual([["upgradeNoteSeenAt"]]);
  });

  it("opens the release notes from What’s new and counts the note seen", async () => {
    const { writes, props } = renderNote();
    await userEvent.click(screen.getByRole("button", { name: "What’s new" }));
    expect(props.onWhatsNew).toHaveBeenCalledTimes(1);
    expect(note()).toBeNull();
    expect(writes.map((write) => Object.keys(write))).toEqual([["upgradeNoteSeenAt"]]);
  });

  it("stays closed while the write is still on its way", async () => {
    const { rerender, props } = renderNote({ writeSettings: () => undefined });
    await userEvent.click(screen.getByRole("button", { name: "Got it" }));
    rerender(<UpgradeNote {...props} settings={{ upgradeNoteSeenAt: null }} />);
    expect(note()).toBeNull();
  });
});
