import { StrictMode, useEffect, useMemo, useState, type JSX } from 'react'
import { createRoot } from 'react-dom/client'
import type { PluginSession } from '@floti-dev/plugin-sdk'
import { useFlotiEvent, useFlotiSettings, useSecret } from '@floti-dev/plugin-sdk/react'
import { CHANNEL, listItems, messageOf, optionsOf, type ItemList } from '../lib/api'
import '../styles.css'

/**
 * The sidebar panel the rail icon opens. Floti draws the header above it, with
 * the manifest's actions in it; everything below is this page.
 */
function Panel(): JSX.Element {
  const settings = useFlotiSettings()
  const [token, requestToken] = useSecret('sample-token')
  const [list, setList] = useState<ItemList | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [service, setService] = useState<string | null>(null)
  const options = useMemo(() => (settings === null ? null : optionsOf(settings)), [settings])
  /** Bumped to read the list again. */
  const [reads, setReads] = useState(0)
  const reload = (): void => setReads((count) => count + 1)

  useEffect(() => {
    if (options === null || token !== 'ready') return undefined
    let live = true
    listItems(options).then(
      (next) => {
        if (!live) return
        setList(next)
        setError(null)
      },
      (failure: unknown) => {
        if (live) setError(messageOf(failure))
      }
    )
    return () => {
      live = false
    }
  }, [options, token, reads])

  // The background refreshed: read the list again.
  useEffect(() => {
    const channel = new BroadcastChannel(CHANNEL)
    channel.onmessage = (event: MessageEvent) => {
      if (event.data === 'refreshed') setReads((count) => count + 1)
    }
    return () => channel.close()
  }, [])

  useFlotiEvent('action', ({ id }) => {
    if (id === 'refresh') reload()
    if (id === 'add') void floti.tabs.open('item', {}, { title: 'New item' })
  })

  useEffect(() => {
    void floti
      .fetch('service:/hello')
      .then((response) => response.json() as Promise<{ message: string; pid: number }>)
      .then(
        (hello) => setService(`${hello.message} (pid ${String(hello.pid)})`),
        (failure: unknown) => setService(`The service did not answer: ${messageOf(failure)}`)
      )
  }, [])

  if (token === 'missing') {
    return (
      <main className="panel">
        <div className="floti-empty" data-sample-empty="token">
          <p className="floti-empty-title">Add the token</p>
          <p className="floti-empty-text">The server wants a bearer token. Floti keeps it; this page never sees it.</p>
          <div className="floti-empty-actions">
            <button type="button" className="floti-button" data-variant="primary" onClick={() => void requestToken()}>
              Add token
            </button>
          </div>
        </div>
      </main>
    )
  }

  return (
    <main className="panel">
      {error !== null ? (
        <div className="floti-empty" data-sample-empty="error">
          <p className="floti-empty-title">No items</p>
          <p className="floti-empty-text">{error}</p>
          <div className="floti-empty-actions">
            <button type="button" className="floti-button" onClick={reload}>
              Try again
            </button>
          </div>
        </div>
      ) : list === null ? (
        <p className="floti-meta pad">Loading…</p>
      ) : (
        <ul className="floti-list" data-sample-items>
          {list.items.map((item) => (
            <li key={item.id}>
              <button
                type="button"
                className="floti-row item"
                data-sample-item={item.id}
                // A click previews the item, as a click on a file in Floti does: the
                // next click replaces it. A double click opens it to stay.
                onClick={() => void floti.tabs.open('item', { id: item.id }, { title: item.title, preview: true })}
                onDoubleClick={() => void floti.tabs.open('item', { id: item.id }, { title: item.title })}
              >
                <span className="floti-dot" data-tone={item.read ? undefined : 'accent'} />
                <span className="grow">
                  <span className="title">{item.title}</span>
                  <span className="floti-meta">{item.read ? 'Read' : 'Unread'}</span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <hr className="floti-rule" />
      <p className="floti-meta pad" data-sample-service>
        {service ?? 'Asking the service…'}
      </p>
      <Sessions />
    </main>
  )
}

/**
 * The sessions Floti started, kept current by the `sessions` event. What a
 * tracker would show as "who is on this": a name, a folder, and whether it is
 * working.
 */
function Sessions(): JSX.Element {
  const [sessions, setSessions] = useState<PluginSession[] | null>(null)
  useEffect(() => {
    let live = true
    floti.sessions.list().then(
      (listed) => {
        if (live) setSessions(listed)
      },
      () => {
        if (live) setSessions([])
      }
    )
    return () => {
      live = false
    }
  }, [])
  useFlotiEvent('sessions', setSessions)

  return (
    <>
      <hr className="floti-rule" />
      <p className="floti-meta pad">Sessions</p>
      {sessions !== null && sessions.length === 0 ? (
        <p className="floti-meta pad" data-sample-sessions-empty>
          None yet.
        </p>
      ) : (
        <ul className="floti-list" data-sample-sessions>
          {(sessions ?? []).map((session) => (
            <li key={session.id} className="floti-row item" data-sample-session={session.name} data-state={session.state}>
              <span className="floti-dot" data-tone={session.activity === 'busy' || session.activity === 'waiting' ? 'accent' : undefined} />
              <span className="grow">
                <span className="title">{session.name}</span>
                <span className="floti-meta" data-sample-session-says>
                  {session.state === 'ended' ? 'Ended' : (session.activity ?? 'Running')} · {session.cwd}
                </span>
              </span>
            </li>
          ))}
        </ul>
      )}
    </>
  )
}

const root = document.getElementById('root')
if (root !== null) {
  createRoot(root).render(
    <StrictMode>
      <Panel />
    </StrictMode>
  )
}
