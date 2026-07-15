import { request } from './client'
import type {
  AuthResponse,
  RefreshResponse,
  RegisterResponse,
  User,
  UsernameAvailability,
} from './generated'

// Auth/session requests must settle: the router waits for the initial refresh before
// entering guest-only routes, so an unbounded half-open POST could otherwise make
// /login and /register unreachable. Long-running non-auth POSTs remain unbounded.
const AUTH_REQUEST_TIMEOUT_MS = 15_000

// The response types are generated from the API's Rust DTOs into `./generated` and
// re-exported here; the request payloads stay hand-written client types.
export type {
  AuthResponse,
  RefreshResponse,
  RegisterResponse,
  User,
  UsernameAvailability,
} from './generated'

// `captcha_token` is the Cloudflare Turnstile token from the widget; optional
// here because it's only produced/required when a Turnstile site key is set (the
// server verifies it, and treats it as absent when CAPTCHA is disabled).
//
// Registration is email-first: the first step takes only the address (the server
// emails a link to finish); the password + display name are chosen in the second
// step (`completeRegistration`), keyed by the emailed token.
export interface RegisterPayload {
  email: string
  /** Sanitized in-app destination to preserve through the emailed completion link. */
  redirect?: string
  captcha_token?: string
}

export interface CompleteRegistrationPayload {
  token: string
  password: string
  /** Optionally claim a username at signup (issue #362); a #XXXX tag is auto-assigned.
   * Omitted/blank leaves the account without a handle until it's chosen later. */
  username?: string | null
  captcha_token?: string
}

export interface LoginPayload {
  email: string
  password: string
  captcha_token?: string
}

export interface VerifyEmailPayload {
  token: string
  captcha_token?: string
}

export interface ResendVerificationPayload {
  email: string
  captcha_token?: string
}

export interface ForgotPasswordPayload {
  email: string
  captcha_token?: string
}

export interface ResetPasswordPayload {
  token: string
  password: string
  captcha_token?: string
}

// Step one of registration. The response is deliberately generic — the same body
// comes back whether the address was new, mid-registration, or already taken (no
// enumeration oracle), and an eligible address receives the completion link by email.
// `completion_token` is `null` when a real email provider is configured; it is
// only non-null when email sending is disabled (dev/e2e), carrying the token so
// the SPA can drive straight to the set-password step. Mints no session.
export function register(payload: RegisterPayload): Promise<RegisterResponse> {
  return request<RegisterResponse>('/api/auth/register', {
    method: 'POST',
    body: payload,
    timeoutMs: AUTH_REQUEST_TIMEOUT_MS,
  })
}

// Step two: spend the emailed (or dev-bypass) token on a password + optional
// display name. Returns a session (access token + refresh cookie), signing the
// new account in.
export function completeRegistration(payload: CompleteRegistrationPayload): Promise<AuthResponse> {
  return request<AuthResponse>('/api/auth/complete-registration', {
    method: 'POST',
    body: payload,
    timeoutMs: AUTH_REQUEST_TIMEOUT_MS,
  })
}

export function login(payload: LoginPayload): Promise<AuthResponse> {
  return request<AuthResponse>('/api/auth/login', {
    method: 'POST',
    body: payload,
    timeoutMs: AUTH_REQUEST_TIMEOUT_MS,
  })
}

export function me(token: string): Promise<{ user: User }> {
  return request<{ user: User }>('/api/auth/me', {
    token,
    timeoutMs: AUTH_REQUEST_TIMEOUT_MS,
  })
}

/** Set or change the signed-in user's username (issue #362). The server allocates a
 * `#XXXX` discriminator; the returned `User` carries the new `username`/`discriminator`/
 * `handle`. A read-only API key is 403; an offensive/invalid/reserved name is 422. */
export function setUsername(token: string, username: string): Promise<User> {
  return request<User>('/api/auth/username', { method: 'PUT', body: { username }, token })
}

/** Whether a candidate username passes the rules (length/charset/reserved/profanity),
 * for the "choose a username" dialog's live feedback. Authed (the dialog is only reachable
 * while signed in); allocates nothing. */
export function checkUsername(token: string, username: string): Promise<UsernameAvailability> {
  return request<UsernameAvailability>(
    `/api/auth/username/available?username=${encodeURIComponent(username)}`,
    { token },
  )
}

// The refresh POST is the one call the whole app can end up awaiting (the router
// guard and every authFetch queue behind it), so unlike other POSTs it gets a
// bounded lifetime — a half-open socket must not hang the session restore
// forever. keepalive lets an in-flight rotation's response deliver its rotated
// Set-Cookie even when the page navigates away mid-request; losing that cookie
// stranded the browser on a dead token (issue #417).
const REFRESH_TIMEOUT_MS = 15_000

export function refresh(): Promise<RefreshResponse> {
  return request<RefreshResponse>('/api/auth/refresh', {
    method: 'POST',
    keepalive: true,
    timeoutMs: REFRESH_TIMEOUT_MS,
  })
}

export function logout(): Promise<void> {
  return request<void>('/api/auth/logout', {
    method: 'POST',
    // Let revocation + the removal Set-Cookie finish if navigation closes the page.
    keepalive: true,
    timeoutMs: AUTH_REQUEST_TIMEOUT_MS,
  })
}

/** Consume an emailed verification token; the user signs in normally after. */
export function verifyEmail(payload: VerifyEmailPayload): Promise<void> {
  return request<void>('/api/auth/verify-email', {
    method: 'POST',
    body: payload,
    timeoutMs: AUTH_REQUEST_TIMEOUT_MS,
  })
}

/** Always resolves generically (204) — the server never reveals whether the
 * address has an account. */
export function resendVerification(payload: ResendVerificationPayload): Promise<void> {
  return request<void>('/api/auth/resend-verification', {
    method: 'POST',
    body: payload,
    timeoutMs: AUTH_REQUEST_TIMEOUT_MS,
  })
}

/** Always resolves generically (204), like `resendVerification`. */
export function forgotPassword(payload: ForgotPasswordPayload): Promise<void> {
  return request<void>('/api/auth/forgot-password', {
    method: 'POST',
    body: payload,
    timeoutMs: AUTH_REQUEST_TIMEOUT_MS,
  })
}

/** Spend an emailed reset token on a new password. Revokes every session. */
export function resetPassword(payload: ResetPasswordPayload): Promise<void> {
  return request<void>('/api/auth/reset-password', {
    method: 'POST',
    body: payload,
    timeoutMs: AUTH_REQUEST_TIMEOUT_MS,
  })
}
