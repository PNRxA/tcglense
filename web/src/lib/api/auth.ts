import { request } from './client'
import type { AuthResponse, RefreshResponse, User } from './generated'

// The response types are generated from the API's Rust DTOs into `./generated` and
// re-exported here; the request payloads stay hand-written client types.
export type { AuthResponse, RefreshResponse, User } from './generated'

export interface RegisterPayload {
  email: string
  password: string
  display_name?: string | null
}

export interface LoginPayload {
  email: string
  password: string
}

export function register(payload: RegisterPayload): Promise<AuthResponse> {
  return request<AuthResponse>('/api/auth/register', { method: 'POST', body: payload })
}

export function login(payload: LoginPayload): Promise<AuthResponse> {
  return request<AuthResponse>('/api/auth/login', { method: 'POST', body: payload })
}

export function me(token: string): Promise<{ user: User }> {
  return request<{ user: User }>('/api/auth/me', { token })
}

export function refresh(): Promise<RefreshResponse> {
  return request<RefreshResponse>('/api/auth/refresh', { method: 'POST' })
}

export function logout(): Promise<void> {
  return request<void>('/api/auth/logout', { method: 'POST' })
}
