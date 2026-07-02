import { request } from './client'
import type { AuthResponse, RefreshResponse, RegisterResponse, User } from './generated'

// The response types are generated from the API's Rust DTOs into `./generated` and
// re-exported here; the request payloads stay hand-written client types.
export type { AuthResponse, RefreshResponse, RegisterResponse, User } from './generated'

export interface RegisterPayload {
  email: string
  password: string
  display_name?: string | null
}

export interface LoginPayload {
  email: string
  password: string
}

export interface VerifyEmailPayload {
  token: string
}

export interface ResendVerificationPayload {
  email: string
}

export interface ForgotPasswordPayload {
  email: string
}

export interface ResetPasswordPayload {
  token: string
  password: string
}

// Registration mints no session (the account must verify its email first), so it
// returns only the created user — signing in happens after the emailed link.
export function register(payload: RegisterPayload): Promise<RegisterResponse> {
  return request<RegisterResponse>('/api/auth/register', { method: 'POST', body: payload })
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

/** Consume an emailed verification token; the user signs in normally after. */
export function verifyEmail(payload: VerifyEmailPayload): Promise<void> {
  return request<void>('/api/auth/verify-email', { method: 'POST', body: payload })
}

/** Always resolves generically (204) — the server never reveals whether the
 * address has an account. */
export function resendVerification(payload: ResendVerificationPayload): Promise<void> {
  return request<void>('/api/auth/resend-verification', { method: 'POST', body: payload })
}

/** Always resolves generically (204), like `resendVerification`. */
export function forgotPassword(payload: ForgotPasswordPayload): Promise<void> {
  return request<void>('/api/auth/forgot-password', { method: 'POST', body: payload })
}

/** Spend an emailed reset token on a new password. Revokes every session. */
export function resetPassword(payload: ResetPasswordPayload): Promise<void> {
  return request<void>('/api/auth/reset-password', { method: 'POST', body: payload })
}
