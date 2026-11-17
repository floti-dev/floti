import type { JSX } from 'react'
import type { AppSettings, Walkthrough } from '@floti/core/types'
import { TourStep } from '@floti/ui'
import type { DesktopBridge } from '../../../../shared/ipc'
import { WALKTHROUGH_DONE } from './walkthrough'

export interface ShellWalkthroughProps {
  /** At a step past the setup window's: step 5. */
  walkthrough: Walkthrough
  bridge: Pick<DesktopBridge, 'invoke'>
  /** Writes the shell's settings, as every other shell write goes. */
  write: (patch: Partial<AppSettings>) => void
  /** The walkthrough is over, with the workspace step 4 chose, if any. */
  onDone: (workspace: string | null) => void
}

/**
 * Step 5 of the guided first run, the last, in the shell's main area beside
 * the rail and sidebar it names. Continue and Skip both mark the walkthrough
 * done, so it shows again only from Settings > General, and leave the shell
 * on its home screen; it starts no session. Any step the shell is handed is
 * drawn as this one.
 */
export function ShellWalkthrough({ walkthrough, bridge, write, onDone }: ShellWalkthroughProps): JSX.Element {
  const finish = (): void => {
    write(WALKTHROUGH_DONE)
    onDone(walkthrough.workspace)
  }
  return (
    <div
      data-shell-walkthrough
      className="h-full overflow-y-auto rounded-island border border-border bg-surface px-6 pt-[26px] pb-[18px]"
    >
      <div className="h-full min-h-[398px]">
        <TourStep
          onSkip={finish}
          // Step 4 is the setup window's: main opens it again there.
          onBack={() => void bridge.invoke('walkthrough:open', { step: 4 })}
          onContinue={finish}
        />
      </div>
    </div>
  )
}
