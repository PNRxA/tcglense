<script setup lang="ts">
import LegalPageLayout from '@/components/legal/LegalPageLayout.vue'
import { usePageMeta } from '@/lib/seo'

usePageMeta({
  title: 'Privacy Policy',
  description: 'What data TCGLense collects, why, and what it never does with it.',
  canonicalPath: '/privacy',
})
</script>

<template>
  <!-- Plain-language policy drafted from actual system behavior — maintainer should review before launch; not legal advice. -->
  <LegalPageLayout
    title="Privacy Policy"
    last-updated="July 4, 2026"
    sibling-label="Terms of Service"
    sibling-to="/terms"
  >
    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">1. What we collect</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        To provide accounts and collections we store: your email address; an optional display name;
        your password as a salted hash (we never store the password itself); your collection,
        wish-list, and saved import-link entries; and timestamps of account activity such as email
        verification.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">2. Cookies and local storage</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        We set one first-party session cookie: an httpOnly refresh cookie that keeps you signed in.
        It is not readable by page scripts and is not used for tracking. Your theme and UI
        preferences are kept in your browser's local storage. There are no advertising or
        third-party tracking cookies.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">3. Rate limiting</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        We use client IP addresses transiently, in memory, to rate-limit sign-in and registration
        endpoints as an anti-abuse measure. They are not stored durably.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">4. Third-party services</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        We use Resend to deliver account emails (registration, verification, password reset) — your
        email address is shared with them for that purpose only. Sign-in and registration forms may
        use Cloudflare Turnstile to filter automated abuse; Turnstile is provided by Cloudflare,
        which processes connection data under its own privacy policy. Card data and prices come from
        Scryfall, TCGCSV, and MTGJSON; card images are served through our own proxy, so browsing the
        catalog does not send your requests to those services directly.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">5. What we never do</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        We do not sell your data, show ads, or share your collection with anyone else. Your
        collection and wish list are visible only to your account.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">6. Retention and deletion</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        Account data is kept while your account exists. To have your account and its data deleted,
        open an issue on the GitHub repository at
        <a
          href="https://github.com/PNRxA/tcglense"
          target="_blank"
          rel="noopener noreferrer"
          class="text-primary underline-offset-4 hover:underline"
        >
          github.com/PNRxA/tcglense</a
        >
        and we will action it.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">7. Security</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        Passwords are hashed with Argon2. Sessions use short-lived access tokens plus a rotating,
        single-use refresh cookie. No system is perfectly secure, but reasonable, modern measures
        are in place — and because the project is open source, you can verify them in the code.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">8. Changes</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        If this policy changes, the "Last updated" date above will change with it.
      </p>
    </section>
  </LegalPageLayout>
</template>
