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
  /**
   * Caller abort signal (e.g. a vue-query cancellation). Honored on any method; on a
   * GET it is composed with the client's own 60s timeout. A caller-initiated abort
   * re-throws the original `AbortError` untouched (so vue-query swallows its own
   * cancellation); a timeout-abort surfaces as `ApiError('Request timed out', 408)`.
   */
  signal?: AbortSignal
}

/**
 * Encode the shared list-endpoint query params in one place. Keys are emitted in a fixed
 * order (page, page_size, q, sort, dir, set, include_related, name, drop) and falsy values
 * are skipped (a 0 page, empty query, or false flag drops out). Returns '' or a leading
 * `?…` string.
 */
export function listQuery(params: {
  page?: number
  pageSize?: number
  q?: string
  sort?: string
  dir?: string
  set?: string
  includeRelated?: boolean
  name?: string
  drop?: string
}): string {
  const search = new URLSearchParams()
  if (params.page) search.set('page', String(params.page))
  if (params.pageSize) search.set('page_size', String(params.pageSize))
  if (params.q) search.set('q', params.q)
  if (params.sort) search.set('sort', params.sort)
  if (params.dir) search.set('dir', params.dir)
  if (params.set) search.set('set', params.set)
  if (params.includeRelated) search.set('include_related', 'true')
  if (params.name) search.set('name', params.name)
  if (params.drop) search.set('drop', params.drop)
  const qs = search.toString()
  return qs ? `?${qs}` : ''
}

export async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const isRaw = options.rawBody != null
  const method = options.method ?? 'GET'
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

  // GET requests get a 60s ceiling composed by hand from one AbortController that fires
  // on either the timeout or the caller's own signal — Safari <17.4 lacks
  // AbortSignal.any/AbortSignal.timeout. Non-GET requests get no unilateral timeout (a
  // large CSV import on a slow uplink must survive) but still honor a caller signal.
  // The timer is always cleared in `finally`.
  const callerSignal = options.signal
  let signal = callerSignal
  let timeoutId: ReturnType<typeof setTimeout> | undefined
  let onCallerAbort: (() => void) | undefined
  let timedOut = false
  if (method === 'GET') {
    const controller = new AbortController()
    signal = controller.signal
    timeoutId = setTimeout(() => {
      timedOut = true
      controller.abort()
    }, 60_000)
    if (callerSignal) {
      if (callerSignal.aborted) {
        controller.abort()
      } else {
        onCallerAbort = () => controller.abort()
        callerSignal.addEventListener('abort', onCallerAbort)
      }
    }
  }

  try {
    const response = await fetch(`${API_URL}${path}`, {
      method,
      headers,
      // Always send/receive the httpOnly refresh cookie (tcglense_refresh).
      credentials: 'include',
      signal,
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
  } catch (err) {
    // A timeout-abort becomes a non-retryable 408 (the retry policy's 4xx rule) so
    // views surface their error state; a caller-initiated abort re-throws untouched so
    // vue-query can swallow its own cancellation instead of seeing an ApiError.
    if (timedOut) {
      throw new ApiError('Request timed out', 408)
    }
    throw err
  } finally {
    if (timeoutId !== undefined) clearTimeout(timeoutId)
    if (onCallerAbort) callerSignal?.removeEventListener('abort', onCallerAbort)
  }
}
