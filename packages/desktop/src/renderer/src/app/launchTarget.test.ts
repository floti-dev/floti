import { describe, expect, it } from 'vitest'
import { LOCAL_HOST_KEY } from '@floti/core/types'
import { launchTarget } from './launchTarget'

describe('launchTarget', () => {
  it('opens this PC’s launcher on the folder in front when that tab is this PC’s', () => {
    expect(launchTarget(LOCAL_HOST_KEY, { host: LOCAL_HOST_KEY, folder: 'C:/work/shop' })).toEqual({
      host: LOCAL_HOST_KEY,
      folder: 'C:/work/shop'
    })
  })

  it('opens this PC’s launcher, on no folder, with another computer’s tab in front', () => {
    expect(launchTarget(LOCAL_HOST_KEY, { host: 'h-build', folder: '/srv/app' })).toEqual({
      host: LOCAL_HOST_KEY,
      folder: null
    })
  })
})
