import type { ClassValue } from 'clsx'
import { clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/**
 * Return `target` only if it is a safe same-origin path, else null. Guards the
 * post-login `?redirect=` query against open redirects (protocol-relative `//host`
 * or backslash tricks `/\host`).
 */
export function safeInternalPath(target: unknown): string | null {
  if (typeof target !== 'string' || target.length > 2_048 || !target.startsWith('/')) return null
  if (target.startsWith('//') || target.includes('\\')) return null
  for (const char of target) {
    const codepoint = char.codePointAt(0) ?? 0
    if (codepoint <= 0x1f || (codepoint >= 0x7f && codepoint <= 0x9f)) return null
  }
  return target
}

/**
 * Return `raw` only if it is an `http://` **loopback** URL (127.0.0.1 / localhost /
 * [::1]), else null. Gates the CLI sign-in redirect (`/cli-login`): the one-time
 * authorization code is only ever handed to a local loopback listener, never an
 * off-origin URL, so a crafted `?redirect_uri=` can't exfiltrate it. `URL.hostname`
 * normalizes the tricky forms (`user@evil`, `127.0.0.1.evil.com`, uppercase, IDN)
 * to a host the exact allow-list rejects.
 */
export function loopbackRedirectUri(raw: unknown): string | null {
  if (typeof raw !== 'string' || !raw) return null
  try {
    const url = new URL(raw)
    // `new URL('http://[::1]:x').hostname` is the bracketed `[::1]`.
    const host = url.hostname
    if (
      url.protocol === 'http:' &&
      (host === '127.0.0.1' || host === 'localhost' || host === '[::1]')
    ) {
      return raw
    }
  } catch {
    // Not a parseable URL — treated as invalid.
  }
  return null
}
