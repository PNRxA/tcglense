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
  if (typeof target !== 'string' || !target.startsWith('/')) return null
  if (target[1] === '/' || target[1] === '\\') return null
  return target
}
