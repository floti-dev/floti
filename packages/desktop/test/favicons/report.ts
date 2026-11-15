/** What the favicons fixture (`main.fixture.ts`) reports to `browser-favicons.test.ts`. */
export interface FaviconsReport {
  /** The `data:` URL the served image makes. */
  expected: string
  /** An icon in reach, asked for directly. */
  direct: string | null
  /** An icon in reach that redirects to another server in reach. */
  redirected: string | null
  /** An icon in reach that redirects to a server out of reach. */
  refused: string | null
  /** An address in reach that answers with HTML. */
  notImage: string | null
  /** The paths each server was asked for, in order. */
  nearAsked: string[]
  alsoAsked: string[]
  farAsked: string[]
}
