import { describe, expect, it } from "vitest";
import { upgradeNoteDue } from "./upgradeNote";

const SET_UP = "2026-08-11T09:00:00.000Z";
const SEEN = "2026-11-13T09:00:00.000Z";

describe("upgradeNoteDue", () => {
  it("is due on a device set up before this start that has not closed it", () => {
    expect(
      upgradeNoteDue({ firstRunCompletedAt: SET_UP, upgradeNoteSeenAt: null }, false),
    ).toBe(true);
  });

  it("is not due again once it has been closed", () => {
    expect(
      upgradeNoteDue({ firstRunCompletedAt: SET_UP, upgradeNoteSeenAt: SEEN }, false),
    ).toBe(false);
  });

  it("is not due on a new install, whose first run is on screen", () => {
    expect(
      upgradeNoteDue({ firstRunCompletedAt: null, upgradeNoteSeenAt: null }, true),
    ).toBe(false);
  });

  it("is not due before the settings are read", () => {
    expect(upgradeNoteDue(null, false)).toBe(false);
  });
});
