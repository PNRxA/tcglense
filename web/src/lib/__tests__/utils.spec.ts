import { describe, it, expect } from 'vitest'

import { safeInternalPath } from '../utils'

describe('safeInternalPath', () => {
  it('accepts same-origin paths', () => {
    expect(safeInternalPath('/profile')).toBe('/profile')
    expect(safeInternalPath('/dashboard?tab=1')).toBe('/dashboard?tab=1')
  })

  it('rejects open-redirect attempts and non-paths', () => {
    expect(safeInternalPath('//evil.com')).toBeNull()
    expect(safeInternalPath('/\\evil.com')).toBeNull()
    expect(safeInternalPath('https://evil.com')).toBeNull()
    expect(safeInternalPath('profile')).toBeNull()
    expect(safeInternalPath(undefined)).toBeNull()
    expect(safeInternalPath(['/a'])).toBeNull()
  })
})
