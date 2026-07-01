// Empty base -> relative '/api/...' URLs, which go through the Vite dev proxy in
// dev and are same-origin in production. Override with VITE_API_URL if needed.
export const API_URL = import.meta.env.VITE_API_URL ?? ''

/** Error thrown for non-2xx responses, carrying the server message and HTTP status. */
export class ApiError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

interface RequestOptions {
  method?: string
  body?: unknown
  token?: string | null
  /**
   * A raw request body (e.g. a `File`/`Blob` for a CSV upload) sent as-is instead of
   * JSON. When set, `body` is ignored and no JSON `Content-Type` is added — pass
   * `contentType` to set one. A `Blob`/`File` is re-readable, so it survives the auth
   * store's single 401-refresh retry.
   */
  rawBody?: BodyInit | null
  /** Content-Type for a `rawBody` upload (e.g. `'text/csv'`). Ignored without `rawBody`. */
  contentType?: string
}

/**
 * Encode the shared list-endpoint query params in one place. Keys are emitted in a fixed
 * order (page, page_size, q, sort, dir, set, include_related) and falsy values are skipped
 * (a 0 page, empty query, or false flag drops out). Returns '' or a leading `?…` string.
 */
export function listQuery(params: {
  page?: number
  pageSize?: number
  q?: string
  sort?: string
  dir?: string
  set?: string
  includeRelated?: boolean
}): string {
  const search = new URLSearchParams()
  if (params.page) search.set('page', String(params.page))
  if (params.pageSize) search.set('page_size', String(params.pageSize))
  if (params.q) search.set('q', params.q)
  if (params.sort) search.set('sort', params.sort)
  if (params.dir) search.set('dir', params.dir)
  if (params.set) search.set('set', params.set)
  if (params.includeRelated) search.set('include_related', 'true')
  const qs = search.toString()
  return qs ? `?${qs}` : ''
}

export async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const isRaw = options.rawBody != null
  const headers: Record<string, string> = {}
  // JSON bodies default to a JSON Content-Type; a raw upload sets its own (or none).
  if (isRaw) {
    if (options.contentType) headers['Content-Type'] = options.contentType
  } else {
    headers['Content-Type'] = 'application/json'
  }
  if (options.token) {
    headers.Authorization = `Bearer ${options.token}`
  }

  const response = await fetch(`${API_URL}${path}`, {
    method: options.method ?? 'GET',
    headers,
    // Always send/receive the httpOnly refresh cookie (tcglense_refresh).
    credentials: 'include',
    body: isRaw
      ? options.rawBody
      : options.body === undefined
        ? undefined
        : JSON.stringify(options.body),
  })

  // 204 No Content and other empty bodies have nothing to parse; tolerate
  // non-JSON bodies (e.g. a proxy error page) rather than throwing on parse.
  const text = await response.text()
  let data: unknown = null
  if (text) {
    try {
      data = JSON.parse(text)
    } catch {
      data = null
    }
  }

  if (!response.ok) {
    const error = (data as { error?: unknown } | null)?.error
    const message =
      typeof error === 'string' ? error : `Request failed with status ${response.status}`
    throw new ApiError(message, response.status)
  }

  return (data ?? undefined) as T
}
