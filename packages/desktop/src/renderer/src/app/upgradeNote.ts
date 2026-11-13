import type { AppSettings } from "@helm/core/types";

/**
 * Whether the upgrade note is due: on a device that was set up before this
 * start (someone coming from an earlier Helm), once setup is not on screen,
 * until it has been closed. Settings not read yet are never due, so the note
 * cannot flash before the window knows.
 */
export function upgradeNoteDue(
  settings: Pick<AppSettings, "firstRunCompletedAt" | "upgradeNoteSeenAt"> | null,
  setupNeeded: boolean,
): boolean {
  return (
    settings !== null &&
    !setupNeeded &&
    settings.firstRunCompletedAt !== null &&
    settings.upgradeNoteSeenAt === null
  );
}
