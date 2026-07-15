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
