import { useCallback, useEffect, useRef, useState } from 'react'
import type { AppSettings, TemplateChoice, TemplatePreview } from '@floti/core'
import { LOCAL_HOST_KEY } from '@floti/core/types'
import type { ClaudeStatus } from '../../../shared/ipc'
import { bridgeFor, desktop } from './bridge'

/**
 * The shell's CLI status and its new-workspace dialog. First run is the
 * guided walkthrough's (`setup/SetupWindow.tsx`, `setup/ShellWalkthrough.tsx`).
 *
 * The CLI status is fetched here and used twice: by Settings, and by the
 * version banner. One request, one answer, so the two surfaces cannot
 * disagree about what version is installed.
 */

export type WorkspaceDialogMode = 'new' | 'convert'

/**
 * Starter's id: the one template Floti ships, built into the host. The
 * picker's default before `template:list` answers; once it has, the default is
 * whichever row the host marks `builtIn`, so a host that names it differently
 * still opens with its built-in chosen.
 */
const STARTER = 'starter'

/** The row to choose: `current` while the listing still has it, else the built-in. */
function chosenFrom(templates: readonly TemplateChoice[], current: string): string {
  if (templates.some((choice) => choice.id === current)) return current
  return templates.find((choice) => choice.builtIn)?.id ?? templates[0]?.id ?? current
}

const sentence = (err: unknown): string => (err instanceof Error ? err.message : String(err))

export interface SetupState {
  /**
   * What the host last answered about Claude Code. Null until it has answered:
   * a read that failed leaves it as it was, because "could not look" is not an
   * answer, and in particular not "not found".
   */
  status: ClaudeStatus | null
  /** Why the last read failed, for the pane's Unknown; null once one succeeded. */
  statusError: string | null
  checking: boolean
  recheck: () => void
  locateClaude: () => void

  /** The create-a-workspace dialog, which is reachable long after first run. */
  dialog: WorkspaceDialogMode | null
  /**
   * The host the dialog creates on: this PC's (`LOCAL_HOST_KEY`), or a host on
   * another computer by its id. Its templates, its suggested folder, the
   * preview and the workspace itself all go through that host's bridge only.
   */
  dialogHost: string
  dialogDir: string
  /** A folder typed for a host on another computer, where there is no picker. */
  typeDialogDir: (dir: string) => void
  dialogProblems: string[]
  creating: boolean
  /** Opens the dialog on `host`, this PC's by default. */
  openDialog: (mode: WorkspaceDialogMode, host?: string) => void
  closeDialog: () => void
  chooseDialogDir: () => void
  /** The dialog owns its mode once open; the preview has to follow it. */
  setDialogMode: (mode: WorkspaceDialogMode) => void
  createWorkspace: (request: {
    mode: WorkspaceDialogMode
    dir: string
    name: string
    template: string
  }) => void

  /** The picker: Starter first, then whatever is in the templates directory. */
  templates: TemplateChoice[]
  templatesDir: string
  templateProblems: string[]
  /** Re-reads them, for when the manager has been authoring over this dialog. */
  refreshTemplates: () => void
  /** Which one is chosen. Reset to Starter every time the dialog opens. */
  template: string
  chooseTemplate: (template: string) => void
  /** What "What gets written" lists. Null until the first answer lands. */
  templatePreview: TemplatePreview | null

  /** Set for the session once the version banner has been dismissed. */
  bannerDismissed: boolean
  dismissBanner: () => void
}

export function useSetup(
  settings: AppSettings | null,
  /** Called when a host's roots have changed and its tree should be rebuilt. */
  onRootsChanged: (host: string) => void,
  /**
   * Called with a workspace that was created whole. Creating one is a step on
   * the way to working in it, so the caller starts a session there rather
   * than leaving somebody on a page.
   */
  onWorkspaceCreated?: (path: string, host: string) => void
): SetupState {
  const [status, setStatus] = useState<ClaudeStatus | null>(null)
  const [statusError, setStatusError] = useState<string | null>(null)
  const [checking, setChecking] = useState(false)
  const [dialog, setDialog] = useState<WorkspaceDialogMode | null>(null)
  const [dialogHost, setDialogHost] = useState<string>(LOCAL_HOST_KEY)
  /** The dialog's host as the last opening left it, for callbacks that outlive a render. */
  const dialogHostRef = useRef<string>(LOCAL_HOST_KEY)
  const [dialogDir, setDialogDir] = useState('')
  const [dialogMode, setDialogMode] = useState<WorkspaceDialogMode>('new')
  const [dialogProblems, setDialogProblems] = useState<string[]>([])
  const [creating, setCreating] = useState(false)
  const [bannerDismissed, setBannerDismissed] = useState(false)
  const [templates, setTemplates] = useState<TemplateChoice[]>([])
  const [templatesDir, setTemplatesDir] = useState('')
  const [templateProblems, setTemplateProblems] = useState<string[]>([])
  const [template, setTemplate] = useState(STARTER)
  const [templatePreview, setTemplatePreview] = useState<TemplatePreview | null>(null)

  /** A host answer replaces what was known; a failure only says it could not look. */
  const answered = useCallback((next: ClaudeStatus) => {
    setStatus(next)
    setStatusError(null)
  }, [])
  const failed = useCallback((err: unknown) => setStatusError(sentence(err)), [])

  /** The button. Shows a spinner, because the user asked and is waiting. */
  const recheck = useCallback(() => {
    setChecking(true)
    void desktop
      .invoke('setup:status')
      .then(answered, failed)
      .finally(() => setChecking(false))
  }, [answered, failed])

  /**
   * Read on mount, and again whenever the chosen CLI changes - however it
   * changed. The picker is one way; a settings write is another, and a banner
   * that only updated for the first would describe a version that is no longer
   * the one desktop would launch.
   *
   * No spinner here: nobody asked for this one, and `checking` is about a
   * button someone is looking at.
   */
  useEffect(() => {
    let cancelled = false
    void desktop.invoke('setup:status').then(
      (next) => {
        if (!cancelled) answered(next)
      },
      (err: unknown) => {
        if (!cancelled) failed(err)
      }
    )
    return () => {
      cancelled = true
    }
  }, [settings?.claudePath, answered, failed])

  const locateClaude = useCallback(() => {
    setChecking(true)
    void desktop
      .invoke('setup:locateClaude')
      .then(
        (next) => {
          answered(next)
          // A newly picked CLI is a reason to look at the banner again, whether it
          // fixed the warning or introduced one.
          setBannerDismissed(false)
        },
        failed
      )
      .finally(() => setChecking(false))
  }, [answered, failed])

  /**
   * Re-reads the picker's rows.
   *
   * Called when the dialog opens, and again when the template manager closes
   * over the top of it - those are the two moments the list can have changed
   * under a dialog somebody is still filling in, and the second one is the
   * whole reason this is a callback rather than three lines inside `openDialog`.
   */
  const refreshTemplates = useCallback(() => {
    void bridgeFor(dialogHostRef.current).invoke('template:list').then(
      (listing) => {
        setTemplates(listing.templates)
        // A template deleted in the manager, or a host whose built-in has
        // another id, leaves the choice on a row that is there.
        setTemplate((current) => chosenFrom(listing.templates, current))
        setTemplatesDir(listing.dir)
        setTemplateProblems(listing.problems)
      },
      // Could not look: the sentence, in the place a template that could not
      // be read is named, rather than a picker that lists none.
      (err: unknown) => setTemplateProblems([sentence(err)])
    )
  }, [])

  const openDialog = useCallback(
    (mode: WorkspaceDialogMode, host: string = LOCAL_HOST_KEY) => {
      setDialogProblems([])
      dialogHostRef.current = host
      setDialogHost(host)
      if (host === LOCAL_HOST_KEY) {
        // Seeded with somewhere plausible so the common case is two clicks: the
        // parent of an existing root for a new workspace, and nothing at all for a
        // conversion, which has to be pointed at a specific folder.
        setDialogDir(mode === 'new' ? (settings?.scanRoots[0] ?? '') : '')
      } else {
        // A host on another computer has no picker here: its folder is typed in
        // its own path form, prefilled with the first folder it suggests.
        setDialogDir('')
        if (mode === 'new') {
          void bridgeFor(host)
            .invoke('roots:suggest')
            .then(
              (suggested) => {
                if (dialogHostRef.current !== host) return
                setDialogDir((typed) => (typed === '' ? (suggested[0] ?? '') : typed))
              },
              // Nothing suggested: the field stays empty for a path to be typed.
              () => undefined
            )
        }
      }
      setDialogMode(mode)
      // Back to Starter every time, deliberately. The picker is not a
      // preference: someone who used a template once has a workspace from it, and
      // the next one is a fresh decision rather than a repeat of the last.
      setTemplate(STARTER)
      // Nothing from the last open: the effect below fills this in, and until
      // it does an empty list is the truthful thing to show.
      setTemplatePreview(null)
      setDialog(mode)
      // Read on open rather than at mount: a template is a directory somebody
      // can add while the desktop is running, and a list fetched once at startup would
      // be stale for the rest of the session.
      refreshTemplates()
    },
    [settings, refreshTemplates]
  )

  /**
   * The file list, re-read whenever the answer would change.
   *
   * Only while the dialog is open, because it is the only thing that shows it.
   * Clearing is `openDialog`'s and `closeDialog`'s job rather than this
   * effect's - a `setState` in an effect body is a cascading render and the
   * lint rule that says so is right here, since "there is no preview" is
   * something those two *know* rather than something to be synchronised.
   */
  useEffect(() => {
    if (dialog === null) return
    let cancelled = false
    void bridgeFor(dialogHost)
      .invoke('template:preview', { template, mode: dialogMode })
      .then((next) => {
        if (!cancelled) setTemplatePreview(next)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [dialog, dialogHost, dialogMode, template])

  const closeDialog = useCallback(() => {
    setDialog(null)
    setDialogProblems([])
    setTemplatePreview(null)
  }, [])

  const chooseDialogDir = useCallback(() => {
    // The picker browses this PC's folders: a host on another computer's path is typed.
    if (dialogHostRef.current !== LOCAL_HOST_KEY) return
    void desktop
      .invoke('path:chooseDirectory', {
        title: dialog === 'convert' ? 'Choose the folder to convert' : 'Create the workspace inside'
      })
      .then(({ path }) => {
        if (path !== null) setDialogDir(path)
      })
  }, [dialog])

  const createWorkspace = useCallback(
    (request: { mode: WorkspaceDialogMode; dir: string; name: string; template: string }) => {
      const host = dialogHostRef.current
      setCreating(true)
      setDialogProblems([])
      void bridgeFor(host)
        .invoke('workspace:create', {
          mode: request.mode,
          dir: request.dir,
          ...(request.name !== '' ? { name: request.name } : {}),
          ...(request.mode === 'new' ? { template: request.template } : {})
        })
        .then((result) => {
          if (result.path === null) {
            setDialogProblems(result.problems)
            return
          }
          // The root was added by the main process in the same call, so the
          // disk is worth another look either way.
          onRootsChanged(host)
          if (result.problems.length > 0) {
            /*
             * A template that applied only partly. The workspace exists and is in
             * the tree behind this dialog, so closing would take the only
             * account of what did *not* get written off the screen with it -
             * there is no second place that list is kept. It stays up, saying
             * so, and Cancel is what dismisses it.
             *
             * The first line names what *did* happen, and it is not decoration:
             * this box is the same red one a refusal uses, and a list of
             * problems with no preamble over a workspace that was in fact created
             * reads as "nothing happened". Honest about the partial write means
             * honest about both halves of it.
             */
            setDialogProblems([
              `${result.path} was created, but not everything in the template could be written:`,
              ...result.problems
            ])
            return
          }
          setDialog(null)
          onWorkspaceCreated?.(result.path, host)
        })
        .catch((err: unknown) => {
          setDialogProblems([err instanceof Error ? err.message : String(err)])
        })
        .finally(() => setCreating(false))
    },
    [onRootsChanged, onWorkspaceCreated]
  )

  return {
    status,
    statusError,
    checking,
    recheck,
    locateClaude,
    dialog,
    dialogHost,
    dialogDir,
    typeDialogDir: setDialogDir,
    dialogProblems,
    creating,
    openDialog,
    closeDialog,
    chooseDialogDir,
    setDialogMode,
    createWorkspace,
    templates,
    templatesDir,
    templateProblems,
    refreshTemplates,
    template,
    chooseTemplate: setTemplate,
    templatePreview,
    bannerDismissed,
    dismissBanner: () => setBannerDismissed(true)
  }
}
