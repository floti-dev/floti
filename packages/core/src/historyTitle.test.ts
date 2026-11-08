import { describe, expect, it } from 'vitest'
import { historyTitle } from './types'

describe('historyTitle', () => {
  const derived = 'add geofencing to the map'

  it('is the name given by hand, over the title its agent gave the session', () => {
    expect(historyTitle({ title: derived, label: 'Geofencing', agentTitle: 'Map: geofencing' })).toBe('Geofencing')
  })

  it('is the title its agent gave the session when nobody named it', () => {
    expect(historyTitle({ title: derived, label: null, agentTitle: 'Map: geofencing' })).toBe('Map: geofencing')
  })

  it('is the title read from its first prompt with neither', () => {
    expect(historyTitle({ title: derived, label: null })).toBe(derived)
  })
})
