import type { JSX } from "react";
import { useCallback, useState } from "react";
import type { AppSettings } from "@helm/core/types";
import { UpgradeNoteDialog } from "@helm/ui";

/**
 * Whether the upgrade note is due: once setup is not on screen, until it has
 * been closed. Which devices it is due on at all is settled as `helm.db`
 * opens (`settleUpgradeNote` in main): only one from before the rename is
 * left unstamped, so a new or reset device never shows it. Settings not read
 * yet are never due, so the note cannot flash before the window knows.
 */
export function upgradeNoteDue(
  settings: Pick<AppSettings, "upgradeNoteSeenAt"> | null,
  setupNeeded: boolean,
): boolean {
  return settings !== null && !setupNeeded && settings.upgradeNoteSeenAt === null;
}

export interface UpgradeNoteProps {
  settings: Pick<AppSettings, "upgradeNoteSeenAt"> | null;
  /** First run is on screen; the note waits for it. */
  setupNeeded: boolean;
  writeSettings: (patch: Partial<AppSettings>) => void;
  /** Opens the release notes in the Browser tab. */
  onWhatsNew: () => void;
}

/**
 * The note someone coming from an earlier Helm sees once, over their own
 * sessions. Closing it in any way, What's new included, writes
 * `upgradeNoteSeenAt`, so it never comes back; it is gone from the window at
 * once, whatever the write does, so a slow or failed write cannot leave it on
 * screen.
 */
export function UpgradeNote({
  settings,
  setupNeeded,
  writeSettings,
  onWhatsNew,
}: UpgradeNoteProps): JSX.Element | null {
  const [closed, setClosed] = useState(false);
  const close = useCallback(() => {
    setClosed(true);
    writeSettings({ upgradeNoteSeenAt: new Date().toISOString() });
  }, [writeSettings]);

  if (closed || !upgradeNoteDue(settings, setupNeeded)) return null;
  return (
    <UpgradeNoteDialog
      onClose={close}
      onWhatsNew={() => {
        close();
        onWhatsNew();
      }}
    />
  );
}
