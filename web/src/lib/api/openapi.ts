import { request } from './client'

/** Load the public OpenAPI document, optionally attaching the current session token. */
export function getOpenApiDocument(token?: string | null): Promise<unknown> {
  return request<unknown>('/api/openapi.json', { token })
}
