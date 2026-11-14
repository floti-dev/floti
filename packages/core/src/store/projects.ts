import type { Project } from '../types'

/**
 * A project as the host's discovery cache gives it to the launcher, which
 * paints from it before a scan finishes.
 */
export interface CachedProject extends Project {
  lastSeenAt: string
  /** The workspace manifest's `template:`, for a workspace row. Null otherwise. */
  template: string | null
}
