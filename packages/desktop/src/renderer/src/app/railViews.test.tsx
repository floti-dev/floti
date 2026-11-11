import { describe, expect, it, vi } from 'vitest'
import { viewRailItems } from './railViews'

describe('the rail’s views', () => {
  it('puts Agents after Profiles, opens the Agents view from it and marks it while the sidebar shows it', () => {
    const onToggle = vi.fn()
    const items = viewRailItems({ shown: 'agents', waiting: false, onToggle })
    expect(items.map((item) => item.label)).toEqual(['Sessions', 'Profiles', 'Agents'])
    const agents = items[2]!
    expect([agents.kind, agents.current, agents.hooks]).toEqual(['view', true, { 'data-open-agents': true }])
    agents.onSelect()
    expect(onToggle).toHaveBeenCalledWith('agents')

    const hidden = viewRailItems({ shown: null, waiting: true, onToggle })
    expect(hidden.map((item) => item.current)).toEqual([false, false, false])
    expect(hidden[0]!.attention).toBe(true)
  })
})
