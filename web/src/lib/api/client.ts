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
}

export async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  }
  if (options.token) {
    headers.Authorization = `Bearer ${options.token}`
  }

  const response = await fetch(`${API_URL}${path}`, {
    method: options.method ?? 'GET',
    headers,
    // Always send/receive the httpOnly refresh cookie (tcglense_refresh).
    credentials: 'include',
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
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
