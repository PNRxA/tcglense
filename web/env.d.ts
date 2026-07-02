/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_API_URL?: string
  /** Cloudflare Turnstile site key. When unset, the CAPTCHA widget is not
   * rendered and no token is sent (the API must have CAPTCHA disabled too). */
  readonly VITE_TURNSTILE_SITE_KEY?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
