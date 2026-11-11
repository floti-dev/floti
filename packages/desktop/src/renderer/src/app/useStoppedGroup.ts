import { useCallback, useMemo, useState } from "react";
import type { RestorableSession } from "@helm/core/types";
import type { StoppedGroup } from "@helm/ui";

/** What the Stopped group asks of the window. */
export interface StoppedGroupDeps {
  /** The conversations running now, by lower-cased id: one of them is back. */
  running: { has: (conversation: string) => boolean };
  /**
   * Resumes a conversation on this PC's host, as `agent` when given. Null
   * when it came back, else the sentence it was refused with.
   */
  resume: (conversationId: string, agent: string | undefined) => Promise<string | null>;
  /** Shows a toast whose Undo runs `undo`. */
  raiseUndo: (title: string, detail: string, undo: () => Promise<string | null>) => void;
}

export interface StoppedGroupState {
  /** The group History draws: null when nothing is left in it. */
  shown: StoppedGroup | null;
  /**
   * Keeps the sessions a stop took that are not back, replacing what the
   * group held: the offer's Not now keeps them all, a restore keeps those it
   * did not bring back.
   */
  keep: (sessions: readonly RestorableSession[]) => void;
  /**
   * Resumes these sessions, each by its conversation, as its agent unless the
   * offer named its agent's problem. Each that comes back leaves the group;
   * one that is refused stays. Null when all came back, else the last
   * refusal's sentence.
   */
  resume: (sessions: readonly RestorableSession[]) => Promise<string | null>;
  /** Takes the group away, with Undo in a toast: nothing asks first. */
  dismiss: () => void;
}

/**
 * The sessions a stop took, at the top of History until each is back or the
 * group is dismissed. The host forgets a restore offer once it is answered,
 * so this window keeps them.
 */
export function useStoppedGroup({ running, resume, raiseUndo }: StoppedGroupDeps): StoppedGroupState {
  const [kept, setKept] = useState<StoppedGroup | null>(null);

  const shown = useMemo((): StoppedGroup | null => {
    if (kept === null) return null;
    const sessions = kept.sessions.filter(
      (lost) => lost.conversationId === undefined || !running.has(lost.conversationId.toLowerCase()),
    );
    return sessions.length === 0 ? null : { ...kept, sessions };
  }, [kept, running]);

  const keep = useCallback((sessions: readonly RestorableSession[]) => {
    setKept(sessions.length === 0 ? null : { at: null, sessions });
  }, []);

  const resumeSessions = useCallback(
    async (sessions: readonly RestorableSession[]): Promise<string | null> => {
      const back = new Set<number>();
      let refused: string | null = null;
      for (const lost of sessions) {
        if (lost.conversationId === undefined) {
          refused = `Floti never learned which conversation ${lost.title ?? lost.name} was, so it cannot be resumed.`;
          continue;
        }
        const answer = await resume(
          lost.conversationId,
          lost.agentProblem === undefined ? lost.agent : undefined,
        );
        if (answer === null) back.add(lost.id);
        else refused = answer;
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
    if (was === null) return;
    setKept(null);
    const count = was.sessions.length;
    raiseUndo(
      "Stopped sessions dismissed",
      `${String(count)} ${count === 1 ? "session" : "sessions"} the stop took ${count === 1 ? "is" : "are"} no longer at the top of History.`,
      async () => {
        setKept(was);
        return null;
      },
    );
  }, [shown, raiseUndo]);

  return { shown, keep, resume: resumeSessions, dismiss };
}
