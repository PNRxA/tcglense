import { describe, it, expect } from 'vitest'

import { loopbackRedirectUri, safeInternalPath } from '../utils'

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

describe('loopbackRedirectUri', () => {
  it('accepts http loopback URLs (any port/path)', () => {
    expect(loopbackRedirectUri('http://127.0.0.1:42229/callback')).toBe(
      'http://127.0.0.1:42229/callback',
    )
    expect(loopbackRedirectUri('http://localhost:8081/callback')).toBe(
      'http://localhost:8081/callback',
    )
    expect(loopbackRedirectUri('http://[::1]:5000/cb')).toBe('http://[::1]:5000/cb')
    // Uppercase host normalizes to loopback and is accepted.
    expect(loopbackRedirectUri('http://LOCALHOST:9000/callback')).toBe(
      'http://LOCALHOST:9000/callback',
    )
  })

  it('rejects every off-loopback / off-origin form', () => {
    // Not loopback at all.
    expect(loopbackRedirectUri('http://evil.com/callback')).toBeNull()
    // Credentials trick: the real host is evil.com.
    expect(loopbackRedirectUri('http://localhost@evil.com/callback')).toBeNull()
    // Subdomain trick: host is 127.0.0.1.evil.com.
    expect(loopbackRedirectUri('http://127.0.0.1.evil.com/callback')).toBeNull()
    // https is not allowed (the CLI listens on plain http loopback).
    expect(loopbackRedirectUri('https://127.0.0.1:42229/callback')).toBeNull()
    // Non-http schemes.
    expect(loopbackRedirectUri('javascript:alert(1)')).toBeNull()
    expect(loopbackRedirectUri('file:///etc/passwd')).toBeNull()
    // Protocol-relative and non-URLs.
    expect(loopbackRedirectUri('//127.0.0.1/callback')).toBeNull()
    expect(loopbackRedirectUri('/callback')).toBeNull()
    expect(loopbackRedirectUri('not a url')).toBeNull()
    // Missing / wrong-typed inputs (e.g. a repeated query param arriving as an array).
    expect(loopbackRedirectUri('')).toBeNull()
    expect(loopbackRedirectUri(undefined)).toBeNull()
    expect(loopbackRedirectUri(['http://127.0.0.1/cb'])).toBeNull()
  })
})
