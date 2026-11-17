import { Fragment, type JSX } from "react";
import {
  LOCAL_HOST_KEY,
  localHostVersionsSentence,
  type LocalHostVersions,
} from "@floti/core/types";
import { cn } from "../lib/cn";

export interface StatusBarProps {
  /**
   * Floti's own live sessions, by what they are doing. `error` counts those
   * whose program reports an error. `idle` counts every other session that is
   * alive and not working or waiting - one that has handed back with a
   * background task still running, and one that has not said anything yet, as
   * well as one that is simply ready.
   */
  sessions: {
    working: number;
    waiting: number;
    error: number;
    /** Command children whose command is still running. */
    running?: number | undefined;
    idle: number;
  };
  /** Brings the next session that is waiting on you to the front. */
  onShowWaiting: () => void;
  /**
   * What kind of build this is when it is not an ordinary install - "dev",
   * "dev · live", "portable" - and null when it is. Each of those changes what
   * the binary is and where the data lives, so it is worth a word; an
   * installed build is the case that needs none.
   */
  mode: string | null;
  /** The app's own version, first on the bar. Null until the app has said. */
  version: string | null;
  /**
   * A newer release than this build, as this PC's host's last check found
   * it, or null when there is none or nothing is known. Structural rather
   * than the desktop package's protocol types: the bar renders a version.
   */
  update: { latest: string } | null;
  /** Opens Settings > Updates, where Update installs it. */
  onOpenUpdate: () => void;
  /** Each plugin's item, in the order the plugins were added. Pressing one opens its panel. */
  plugins?: readonly StatusPluginItem[] | undefined;
  /**
   * The hosts sessions run in and what quitting the desktop does on each, this PC
   * first. Pressing one opens the setting that decides.
   */
  hosts?: readonly StatusHostItem[] | undefined;
}

export interface StatusHostItem {
  /** `local` for this PC's own host, a remote host's id otherwise. */
  id: string;
  /** What the bar calls the host: "This PC" for this PC's own. */
  name: string;
  /** Whether quitting the desktop ends the sessions it started there. */
  quitEnds: boolean;
  /** The link to the host ended and is trying again by itself. Nothing there ended. */
  retrying?: boolean | undefined;
  /**
   * The host's ssh or command could not reach it: the last line it wrote,
   * unedited, and the line to try by hand in a terminal. The item says the
   * host is not reachable, and its hover text holds both lines.
   */
  unreachable?: { lastLine: string; tryLine: string } | null | undefined;
  /**
   * How long the window's last ping to a connected host on another computer
   * took to come back, in milliseconds: what a keystroke there costs. Null
   * before its first answer. This PC's own host is never given one a figure.
   */
  roundTripMs?: number | null | undefined;
  /**
   * This PC's host only: which of it and this desktop is behind, said in warning
   * tone; pressing it opens Settings > Hosts, which says what to do. A host
   * that cannot be used says so in the window's notice, with its action.
   */
  versions?: LocalHostVersions | null | undefined;
  /**
   * This PC's host only: Floti is restarting it onto its own, newer floti.
   * The item says the host is updating in place of anything else, with no
   * warning tone and no action: it needs nothing from the user.
   */
  updating?: boolean | undefined;
  /** Opens the setting where what quitting does there is set. */
  onOpen: () => void;
}

export interface StatusPluginItem {
  /** The plugin's id. */
  id: string;
  /** The plugin's name, said in the tooltip when the item has none of its own. */
  name: string;
  text: string;
  tone: "neutral" | "accent" | "success" | "warn" | "danger";
  tooltip: string | null;
  onSelect: () => void;
}

const PLUGIN_TONE: Record<StatusPluginItem["tone"], string> = {
  neutral: "text-fg-subtle hover:text-fg",
  accent: "text-accent-text",
  success: "text-success",
  warn: "text-warn",
  danger: "text-danger",
};

/**
 * The bottom strip. On the left, which Floti this is - the version, the build
 * when it is not an ordinary install, a newer release when there is one -
 * then what your sessions are doing, then the hosts they run in. On the
 * right, each plugin's item.
 *
 * The version leads because it is what somebody reads the bar for when
 * something is off, and it is a fixed-width fact that does not move. Which
 * agent CLI a session runs, and its version, are in Settings: the bar names
 * nothing that belongs to one agent.
 */
export function StatusBar({
  sessions,
  onShowWaiting,
  mode,
  version,
  update,
  onOpenUpdate,
  plugins = [],
  hosts = [],
}: StatusBarProps): JSX.Element {
  const running = sessions.running ?? 0;
  const total =
    sessions.working +
    sessions.waiting +
    sessions.error +
    running +
    sessions.idle;
  return (
    // No island and no border: the status bar is the one strip that sits
    // directly on the canvas (DESIGN.md), a caption under the islands rather
    // than a panel of its own.
    <footer className="flex h-[26px] shrink-0 items-center gap-3 px-3.5 text-[11px] text-fg-subtle tabular-nums">
      {version !== null && (
        <span data-status-version className="shrink-0">
          Floti {version}
        </span>
      )}

      {mode !== null && (
        <span
          data-status-mode={mode}
          className="shrink-0 rounded-sm border border-border-strong px-1.5 leading-[16px] text-fg-muted"
        >
          {mode}
        </span>
      )}

      {/* Beside the version, because that is the thing it is about. The
          accent as *text*, never as a fill: a newer release is worth noticing
          and is not a warning - colouring it `warn` would put it in the
          language of things you have to fix. This is an offer. */}
      {update !== null && (
        <button
          type="button"
          data-update-available={update.latest}
          title={`Floti ${update.latest} was released. Opens Settings > Updates, where Update installs it.`}
          onClick={() => onOpenUpdate()}
          className={cn(
            "-mx-1 shrink-0 rounded px-1 text-accent underline decoration-dotted",
            "underline-offset-[3px] transition-colors hover:bg-hover",
          )}
        >
          {update.latest} available
        </button>
      )}

      {/* The rule closes the version group, so it is drawn only while that
          group has something in it. */}
      {(version !== null || mode !== null || update !== null) && <Divider />}

      {total === 0 ? (
        <span data-status-sessions="none">No sessions running</span>
      ) : (
        <>
          {sessions.working > 0 && (
            <span
              data-status-sessions="working"
              className="flex shrink-0 items-center gap-1.5"
            >
              <span aria-hidden className="size-1.5 rounded-full bg-accent" />
              {sessions.working} working
            </span>
          )}
          {/* The one segment that is a button: "needs you" is a request, and
              the answer to it is to go there. Each press brings the next one
              forward. */}
          {sessions.waiting > 0 && (
            <button
              type="button"
              data-status-sessions="waiting"
              onClick={onShowWaiting}
              title="Show the session that is waiting on you"
              className="-mx-1 flex shrink-0 items-center gap-1.5 rounded px-1 text-warn transition-colors hover:bg-hover"
            >
              <span aria-hidden className="size-1.5 rounded-full bg-warn" />
              {sessions.waiting} needs you
            </button>
          )}
          {sessions.error > 0 && (
            <span
              data-status-sessions="error"
              className="flex shrink-0 items-center gap-1.5 text-danger"
            >
              <span aria-hidden className="size-1.5 rounded-full bg-danger" />
              {sessions.error} in error
            </span>
          )}
          {/* A command a session started and you watch: it runs, and takes
              no turns, so it is neither working nor idle. */}
          {running > 0 && (
            <span
              data-status-sessions="running"
              className="flex shrink-0 items-center gap-1.5"
            >
              <span aria-hidden className="size-1.5 rounded-full bg-success" />
              {running} running
            </span>
          )}
          {sessions.idle > 0 && (
            <span
              data-status-sessions="idle"
              className="flex shrink-0 items-center gap-1.5"
            >
              <span
                aria-hidden
                className="size-1.5 rounded-full border-[1.5px] border-success"
              />
              {sessions.idle} idle
            </span>
          )}
        </>
      )}

      {/* Where the sessions live and what a quit does to them, said before
          anybody quits: a quit should never do what you did not expect. One
          item per host, this PC first, each behind a rule of its own. */}
      {hosts.map((host) => {
        const unreachable = host.unreachable ?? null;
        const away = host.retrying === true || unreachable !== null;
        // This PC's host is no network away, so it never shows a figure.
        const roundTrip =
          away || host.id === LOCAL_HOST_KEY
            ? null
            : (host.roundTripMs ?? null);
        if (host.updating === true) {
          return (
            <Fragment key={host.id}>
              <Divider />
              <span
                data-status-host="updating"
                data-status-host-id={host.id}
                title={`Floti is updating the host on ${host.name} to its own floti. Its sessions carry over.`}
                className="flex shrink-0 items-center gap-1.5"
              >
                <span
                  aria-hidden
                  className="size-1.5 rounded-full border-[1.5px] border-fg-subtle"
                />
                {host.name} · updating host&hellip;
              </span>
            </Fragment>
          );
        }
        const versions = away ? null : (host.versions ?? null);
        if (versions !== null) {
          return (
            <Fragment key={host.id}>
              <Divider />
              <button
                type="button"
                data-status-host={
                  versions.older === "host" ? "older" : "update-desktop"
                }
                data-status-host-id={host.id}
                onClick={host.onOpen}
                title={`${localHostVersionsSentence(versions)} Opens Settings.`}
                className="-mx-1 flex shrink-0 items-center gap-1.5 rounded px-1 text-warn transition-colors hover:bg-hover"
              >
                <span
                  aria-hidden
                  className="size-1.5 rounded-full border-[1.5px] border-warn"
                />
                {host.name} ·{" "}
                {versions.older === "desktop" ? "update Floti" : "older host"}
              </button>
            </Fragment>
          );
        }
        return (
          <Fragment key={host.id}>
            <Divider />
            <button
              type="button"
              data-status-host={
                unreachable !== null
                  ? "unreachable"
                  : away
                    ? "retrying"
                    : host.quitEnds
                      ? "ends"
                      : "keeps"
              }
              data-status-host-id={host.id}
              data-status-round-trip={roundTrip ?? undefined}
              onClick={host.onOpen}
              title={
                unreachable !== null
                  ? `Floti cannot reach ${host.name}. It said:
${unreachable.lastLine}

` +
                    `Try it by hand in a terminal:
${unreachable.tryLine}

` +
                    "Nothing running there ended, and Floti keeps trying. Opens Settings."
                  : (away
                      ? `Floti lost its connection to ${host.name} and is trying again. Nothing running there ended. `
                      : `Sessions run in the host on ${host.name}. ` +
                        (roundTrip === null
                          ? ""
                          : `A keystroke typed into a session on ${host.name} takes ${String(roundTrip)} ms to come back. `)) +
                    `Quitting Floti ${host.quitEnds ? "ends" : "keeps running"} the ones it started there. Opens Settings.`
              }
              className={cn(
                "-mx-1 flex shrink-0 items-center gap-1.5 rounded px-1 transition-colors hover:bg-hover hover:text-fg",
                away && "text-warn",
              )}
            >
              {away && (
                <span
                  aria-hidden
                  className="size-1.5 rounded-full border-[1.5px] border-warn"
                />
              )}
              {host.name} ·{" "}
              {unreachable !== null
                ? "not reachable"
                : away
                  ? "not connected, retrying"
                  : `${roundTrip === null ? "" : `${String(roundTrip)} ms · `}quit ${host.quitEnds ? "ends" : "keeps"} sessions`}
            </button>
          </Fragment>
        );
      })}

      <span className="flex-1" />

      {/* A plugin's word, in its own tone, never more than a line of caption:
          the bar is chrome, and a plugin is one voice on it. */}
      {plugins.map((item) => (
        <button
          key={item.id}
          type="button"
          data-status-plugin={item.id}
          title={item.tooltip ?? item.name}
          onClick={item.onSelect}
          className={cn(
            "-mx-1 max-w-[220px] shrink-0 truncate rounded px-1 transition-colors hover:bg-hover",
            PLUGIN_TONE[item.tone],
          )}
        >
          {item.text}
        </button>
      ))}
    </footer>
  );
}

/** A hairline between the groups on the left. */
function Divider(): JSX.Element {
  return <span aria-hidden className="h-2.5 w-px shrink-0 bg-border-strong" />;
}
