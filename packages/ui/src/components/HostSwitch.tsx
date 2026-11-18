import { useLayoutEffect, useRef, useState, type JSX } from 'react'
import { cn } from '../lib/cn'
import { SEGMENT_ON } from '../lib/segmented'
import { CaretIcon } from './icons'
import { Menu, type MenuEntry } from './Menu'
import type { LauncherHost } from './NewSessionDialog'

/** A host a surface can show, and whether the window can reach it now. */
export interface HostChoiceHost extends LauncherHost {
  /** The window is trying to reach it and cannot: said in danger in the menu, and still pickable. */
  away?: boolean | undefined
}

/**
 * Which host a surface shows, and the hosts it can show: This PC first, then
 * each host on another computer in the order Settings > Hosts lists them.
 * History, Files, Agents and Settings' Floti host group each take one, and
 * keep the host they show in the window only.
 */
export interface HostChoice {
  hosts: readonly HostChoiceHost[]
  /** The id of the host shown, one of `hosts`. */
  host: string
  onHostChange: (host: string) => void
}

export interface HostSwitchProps extends Partial<HostChoice> {
  /** What the surface is, for the control's accessible name: "History" reads as "History host". */
  surface: string
  /** `small` for a caps label's row, such as Settings' Floti host. */
  size?: 'regular' | 'small' | undefined
  className?: string | undefined
}

/** A segmented control holds at most this many hosts; more are a menu whatever the room. */
export const SEGMENTED_MOST = 3

/**
 * The host a surface shows, named, with the others to switch to: This PC's
 * name in sans, a remote host's in mono, as everywhere a host is named.
 * Drawn only while there is more than one host to choose from, so with only
 * this PC a surface looks as it always has.
 *
 * A segmented control (DESIGN.md 4) while it has at most three hosts and they
 * fit where it stands; otherwise a button naming the shown host, with its
 * state dot and a caret, that opens a menu of every host with its state. The
 * room is measured, not counted: a hidden copy of the segmented control gives
 * the width it needs, and the switch takes at most that much, shrinking where
 * its row has less. So one window changes form as its sidebar narrows.
 */
export function HostSwitch({
  hosts,
  host,
  onHostChange,
  surface,
  size = 'regular',
  className
}: HostSwitchProps): JSX.Element | null {
  const boxRef = useRef<HTMLSpanElement>(null)
  const measureRef = useRef<HTMLSpanElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)
  /** The width the segmented control needs, and whether the switch has it. */
  const [room, setRoom] = useState<{ needs: number; fits: boolean }>({ needs: 0, fits: true })
  const [open, setOpen] = useState<DOMRect | null>(null)
  const counted = hosts !== undefined && hosts.length > SEGMENTED_MOST
  const namesKey = hosts?.map((each) => `${each.id}\n${each.name}`).join('\n') ?? ''

  useLayoutEffect(() => {
    const box = boxRef.current
    const measure = measureRef.current
    if (counted || box === null || measure === null) return
    const check = (): void => {
      const needs = measure.offsetWidth
      const fits = box.offsetWidth >= needs
      setRoom((was) => (was.needs === needs && was.fits === fits ? was : { needs, fits }))
    }
    check()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(check)
    observer.observe(box)
    observer.observe(measure)
    return () => observer.disconnect()
  }, [counted, namesKey, size])

  if (hosts === undefined || hosts.length < 2 || host === undefined) return null
  const shown = hosts.find((each) => each.id === host)
  const small = size === 'small'
  const name = `${surface} host`
  const segmented = !counted && room.fits
  const pick = (next: string): void => {
    if (next !== host) onHostChange?.(next)
  }

  const segments = (live: boolean): JSX.Element => (
    <span
      role={live ? 'radiogroup' : undefined}
      aria-label={live ? name : undefined}
      title={live && shown !== undefined ? `Showing ${shown.name}’s ${surface.toLowerCase()}` : undefined}
      className="flex shrink-0 gap-0.5 rounded-well border border-border bg-surface-sunken p-0.5"
    >
      {hosts.map((each) => (
        <button
          key={each.id}
          type="button"
          role={live ? 'radio' : undefined}
          aria-checked={live ? each.id === host : undefined}
          tabIndex={live ? undefined : -1}
          data-host-choice={live ? each.id : undefined}
          onClick={live ? () => pick(each.id) : undefined}
          className={cn(
            'rounded-raised whitespace-nowrap transition-colors',
            small ? 'px-[7px] text-[10.5px] leading-[18px]' : 'px-2 py-0.5 text-[11px]',
            each.remote && (small ? 'font-mono' : 'font-mono text-[10.5px]'),
            each.id === host ? SEGMENT_ON : 'text-fg-muted hover:text-fg'
          )}
        >
          {each.name}
        </button>
      ))}
    </span>
  )

  const entries: MenuEntry[] = [
    { kind: 'heading', id: 'heading', label: `${surface} of` },
    ...hosts.map(
      (each): MenuEntry => ({
        kind: 'item',
        id: each.id,
        label: each.name,
        mono: each.remote,
        checked: each.id === host,
        icon: <StateDot away={each.away === true} />,
        hint: each.away === true ? 'out of reach' : 'connected',
        hintSans: true,
        hintTone: each.away === true ? 'danger' : undefined
      })
    )
  ]

  return (
    <span
      ref={boxRef}
      data-host-switch={surface.toLowerCase()}
      data-host-shown={host}
      data-host-switch-form={segmented ? 'segmented' : 'menu'}
      // At most the width the segmented control needs, and less where the row
      // has less: what the switch then has is what decides its form.
      style={counted || room.needs === 0 ? undefined : { width: room.needs }}
      className={cn('relative flex min-w-0 shrink items-center', className)}
    >
      {!counted && (
        <span
          ref={measureRef}
          aria-hidden
          inert
          data-host-switch-measure
          className="pointer-events-none invisible absolute top-0 left-0 flex"
        >
          {segments(false)}
        </span>
      )}
      {segmented ? (
        segments(true)
      ) : (
        <button
          ref={buttonRef}
          type="button"
          aria-label={name}
          aria-haspopup="listbox"
          aria-expanded={open !== null}
          data-host-menu-button
          title={shown === undefined ? undefined : `Showing ${shown.name}’s ${surface.toLowerCase()}`}
          onClick={() => {
            if (open !== null) {
              setOpen(null)
              return
            }
            const rect = buttonRef.current?.getBoundingClientRect()
            if (rect !== undefined) setOpen(rect)
          }}
          className={cn(
            'flex max-w-[150px] min-w-0 items-center gap-1.5 rounded-well border bg-surface-sunken pr-1.5 pl-[7px] text-fg transition-colors',
            small ? 'h-5 text-[10.5px]' : 'h-6 text-[11px]',
            open !== null ? 'border-border-strong bg-hover' : 'border-border hover:bg-hover'
          )}
        >
          <StateDot away={shown?.away === true} />
          <span className={cn('min-w-0 truncate', shown?.remote === true && 'font-mono')}>{shown?.name ?? host}</span>
          <CaretIcon width={9} height={9} className="shrink-0 rotate-90 text-fg-subtle" />
        </button>
      )}
      {open !== null && (
        <Menu
          label={name}
          role="listbox"
          entries={entries}
          at={{ below: open }}
          anchorRef={buttonRef}
          minWidth={240}
          onSelect={pick}
          onDismiss={() => setOpen(null)}
        />
      )}
    </span>
  )
}

/** A host's state: a filled `success` dot while connected, a `danger` ring while out of reach. */
function StateDot({ away }: { away: boolean }): JSX.Element {
  return (
    <span
      aria-hidden
      data-host-state={away ? 'away' : 'connected'}
      className={cn('size-[7px] shrink-0 rounded-full', away ? 'border-[1.5px] border-danger' : 'bg-success')}
    />
  )
}
