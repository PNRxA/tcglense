import { request } from './client'
import type { PublicConfig } from './generated'

export type { PublicConfig } from './generated'

/**
 * Public, unauthenticated runtime config the SPA needs before it can render the
 * auth forms (currently the Cloudflare Turnstile site key). Fetched at runtime so
 * the shipped bundle needs no rebuild to change it. No token — a plain public GET.
 */
export function getConfig(): Promise<PublicConfig> {
  return request<PublicConfig>('/api/config')
}
