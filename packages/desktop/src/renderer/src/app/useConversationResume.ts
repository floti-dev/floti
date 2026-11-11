import { useCallback, useRef, useState } from "react";
import type { SessionRecord } from "@helm/core";
import { bridgeFor } from "./bridge";
import { readable } from "./errors";

/** What a resume from History asks of the window. */
export interface ConversationResumeDeps {
  /** The grid the new session starts at. */
  grid: () => { cols: number; rows: number };
  /** Takes the session the host started on `host` and places it in a tab. */
  adopt: (host: string, record: SessionRecord) => void;
}

export interface ConversationResume {
  /** The key of the resume in flight (a session's id, or a group's), or null. */
  resuming: string | null;
  /**
   * Runs `task` as the one resume in flight, under `key`. Null, and `task` not
   * run, while another is in flight, so a second Enter or click sends nothing
   * while the first is on its way.
   */
  exclusive: <T>(key: string, task: () => Promise<T>) => Promise<T> | null;
  /**
   * Resumes a conversation on `host`, in the folder it ran in, as `agent`
   * when given, and places the session: the one it started, or the one the
   * host says already runs that conversation (`alreadyRunning`), which it
   * starts no second time. Resolves to null when either is shown, else the
   * sentence it was refused with. It does not wait for one in flight;
   * `exclusive` does.
   */
  resumeOn: (host: string, sessionId: string, agent: string | undefined) => Promise<string | null>;
}

/**
 * History's resumes, from the palette, its Stopped group and a conversation
 * tab: on the host the conversation is on, through that host's bridge, one
 * at a time.
 */
export function useConversationResume({ grid, adopt }: ConversationResumeDeps): ConversationResume {
  const [resuming, setResuming] = useState<string | null>(null);
  // Read synchronously, so two presses before a render still start one.
  const flight = useRef<string | null>(null);

  const exclusive = useCallback(<T,>(key: string, task: () => Promise<T>): Promise<T> | null => {
    if (flight.current !== null) return null;
    flight.current = key;
    setResuming(key);
    return task().finally(() => {
      flight.current = null;
      setResuming(null);
    });
  }, []);

  const resumeOn = useCallback(
    async (host: string, sessionId: string, agent: string | undefined): Promise<string | null> => {
      try {
        const { cols, rows } = grid();
        const { session: record } = await bridgeFor(host).invoke("history:resume", {
          sessionId,
          ...(agent === undefined || agent === "" ? {} : { agent }),
          cols,
          rows,
        });
        adopt(host, record);
        return null;
      } catch (err: unknown) {
        return readable(err);
      }
    },
    [grid, adopt],
  );

  return { resuming, exclusive, resumeOn };
}
