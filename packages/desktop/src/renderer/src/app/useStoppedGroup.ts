import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { RestorableSession } from "@helm/core/types";
import type { StoppedGroup } from "@helm/ui";

/** What the Stopped group asks of the window. */
export interface StoppedGroupDeps {
  /**
   * The conversations running now on this PC's host, whose sessions these
   * are, by lower-cased id: one of them is back.
   */
  running: { has: (conversation: string) => boolean };
  /**
   * Resumes a conversation on this PC's host, as `agent` when given. Null
   * when it came back, else the sentence it was refused with.
   */
  resume: (conversationId: string, agent: string | undefined) => Promise<string | null>;
  /** Shows a toast whose Undo runs `undo`. */
  raiseUndo: (title: string, detail: string, undo: () => Promise<string | null>) => void;
  /** The sentence the last read of the restore offer failed with, drawn in the group; null when it read. */
  unread: string | null;
  /** Forgets that the read failed. */
  dismissUnread: () => void;
  /** Reads the offer again: the host still holds what it could not say. */
  readAgain: () => void;
}

export interface StoppedGroupState {
  /** The group History draws: null when nothing is left in it. */
  shown: StoppedGroup | null;
  /**
   * Keeps the sessions a stop took that are not back, added to what the
   * group holds, by id: the offer's Not now keeps them all, a restore keeps
   * those it did not bring back, and a second stop's join the first's. `at`
   * is when the host found them lost (epoch ms), which heads the group, or
   * null when it did not say; the latest one known heads it.
   */
  keep: (sessions: readonly RestorableSession[], at: number | null) => void;
  /**
   * Resumes these sessions, each by its conversation, as its agent unless the
   * offer named its agent's problem. Each that comes back leaves the group;
   * one that is refused stays. Null when all came back, else the last
   * refusal's sentence.
   */
  resume: (sessions: readonly RestorableSession[]) => Promise<string | null>;
  /**
   * Takes the group away, a failed read with it, with Undo in a toast:
   * nothing asks first. Undo brings back what it showed but those that have
   * run since, and reads the offer again when a read had failed.
   */
  dismiss: () => void;
}

/**
 * The sessions a stop took, at the top of History until each is back or the
 * group is dismissed. The host forgets a restore offer once it is answered,
 * so this window keeps them.
 */
export function useStoppedGroup({
  running,
  resume,
  raiseUndo,
  unread,
  dismissUnread,
  readAgain,
}: StoppedGroupDeps): StoppedGroupState {
  const [kept, setKept] = useState<StoppedGroup | null>(null);
  /**
   * The conversations of dismissed sessions, and those of them that have run
   * since: back, so an Undo does not return them.
   */
  const watching = useRef(new Set<string>());
  const seenBack = useRef(new Set<string>());
  useEffect(() => {
    for (const conversation of watching.current) {
      if (running.has(conversation)) seenBack.current.add(conversation);
    }
  }, [running]);

  // A session seen running is back, wherever it was resumed: it leaves the
  // group for good, so it does not return there when it exits.
  // Pruned as it is drawn, so the first render that sees one running is the
  // last that holds it.
  const shown = useMemo((): StoppedGroup | null => {
    if (kept === null) return null;
    const sessions = kept.sessions.filter((lost) => !isRunning(lost, running));
    if (sessions.length === kept.sessions.length) return kept;
    return sessions.length === 0 ? null : { ...kept, sessions };
  }, [kept, running]);
  if (shown !== kept) setKept(shown);

  const keep = useCallback((sessions: readonly RestorableSession[], at: number | null) => {
    setKept((current) => merged(current, { at, sessions }));
  }, []);

  const resumeSessions = useCallback(
    async (sessions: readonly RestorableSession[]): Promise<string | null> => {
      const back = new Set<number>();
      let refused: string | null = null;
      // Two stops can each hold a copy of one conversation: it is resumed
      // once, and every copy follows that answer.
      const answered = new Map<string, string | null>();
      for (const lost of sessions) {
        if (lost.conversationId === undefined) {
          refused = `Floti never learned which conversation ${lost.title ?? lost.name} was, so it cannot be resumed.`;
          continue;
        }
        const conversation = lost.conversationId.toLowerCase();
        let answer = answered.get(conversation);
        if (answer === undefined) {
          answer = await resume(
            lost.conversationId,
            lost.agentProblem === undefined ? lost.agent : undefined,
          );
          answered.set(conversation, answer);
          if (answer !== null) refused = answer;
        }
        if (answer === null) back.add(lost.id);
      }
      setKept((current) => {
        if (current === null) return null;
        const left = current.sessions.filter((lost) => !back.has(lost.id));
        return left.length === 0 ? null : { ...current, sessions: left };
      });
      return refused;
    },
    [resume],
  );

  const dismiss = useCallback(() => {
    const was = shown;
    const hadUnread = unread !== null;
    if (was === null && !hadUnread) return;
    setKept(null);
    if (hadUnread) dismissUnread();
    // A conversation dismissed again was lost again since it was last back:
    // only running from now on keeps it out of this Undo.
    for (const lost of was?.sessions ?? []) {
      if (lost.conversationId === undefined) continue;
      const conversation = lost.conversationId.toLowerCase();
      watching.current.add(conversation);
      seenBack.current.delete(conversation);
    }
    const count = was?.sessions.length ?? 0;
    raiseUndo(
      "Stopped sessions dismissed",
      count === 0
        ? "The sessions the stop took that could not be read are no longer at the top of History."
        : `${String(count)} ${count === 1 ? "session" : "sessions"} the stop took ${count === 1 ? "is" : "are"} no longer at the top of History.`,
      async () => {
        if (was !== null) {
          const sessions = was.sessions.filter(
            (lost) =>
              lost.conversationId === undefined ||
              !seenBack.current.has(lost.conversationId.toLowerCase()),
          );
          setKept((current) => merged(current, { ...was, sessions }));
        }
        if (hadUnread) readAgain();
        return null;
      },
    );
  }, [shown, unread, raiseUndo, dismissUnread, readAgain]);

  return { shown, keep, resume: resumeSessions, dismiss };
}

function isRunning(lost: RestorableSession, running: StoppedGroupDeps["running"]): boolean {
  return lost.conversationId !== undefined && running.has(lost.conversationId.toLowerCase());
}

/** `adding` joined to `current` by session id, the newer copy of each kept; null when nothing is left. */
function merged(current: StoppedGroup | null, adding: StoppedGroup): StoppedGroup | null {
  const byId = new Map((current?.sessions ?? []).map((lost) => [lost.id, lost]));
  for (const lost of adding.sessions) byId.set(lost.id, lost);
  if (byId.size === 0) return null;
  const times = [current?.at ?? null, adding.at].filter((at): at is number => at !== null);
  return { at: times.length === 0 ? null : Math.max(...times), sessions: [...byId.values()] };
}
