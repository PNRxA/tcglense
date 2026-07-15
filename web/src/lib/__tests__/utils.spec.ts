import { describe, it, expect } from 'vitest'

import { safeInternalPath } from '../utils'

describe('safeInternalPath', () => {
  it('accepts same-origin paths', () => {
    expect(safeInternalPath('/profile')).toBe('/profile')
    expect(safeInternalPath('/collection?tab=1')).toBe('/collection?tab=1')
  })

  it('rejects open-redirect attempts and non-paths', () => {
    expect(safeInternalPath('//evil.com')).toBeNull()
    expect(safeInternalPath('/\\evil.com')).toBeNull()
    expect(safeInternalPath('/safe/\\evil.com')).toBeNull()
    expect(safeInternalPath('/safe\nnext')).toBeNull()
    expect(safeInternalPath(`/${'a'.repeat(2_048)}`)).toBeNull()
    expect(safeInternalPath('https://evil.com')).toBeNull()
    expect(safeInternalPath('profile')).toBeNull()
    expect(safeInternalPath(undefined)).toBeNull()
    expect(safeInternalPath(['/a'])).toBeNull()
  })
})
