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
  <LegalPageLayout
    title="Privacy Policy"
    last-updated="July 15, 2026"
    sibling-label="Terms of Service"
    sibling-to="/terms"
  >
    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">1. What we collect</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        To provide accounts and collections we store your email address; your password as a salted
        hash (never the password itself); an optional public username; your collection, wish-list,
        decks, folders, and saved external-collection identifiers; and account timestamps such as
        creation, verification, and last API-key use. If you create an API key, we store its name,
        scope, prefix, and a one-way hash of the secret. The full key is shown only when it is
        created.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">2. Cookies and local storage</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        We set one first-party session cookie: an httpOnly refresh cookie that keeps you signed in.
        It is not readable by page scripts or used for tracking. The short-lived access token is
        held in page memory. Theme, card-size, and other display preferences are kept in your
        browser's local storage. We do not use advertising or analytics cookies.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">3. Rate limiting</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        We use client IP addresses for short periods to rate-limit sign-in and registration and to
        verify CAPTCHA challenges. Rate-limit entries live in server memory or a short-lived cache
        and expire automatically. Our hosting, content-delivery, and security providers may also
        process connection and request metadata to deliver and protect the service.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">4. Third-party services</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        We use Resend to deliver account emails, so your email address and the content of the account
        message are sent to Resend. Cloudflare provides CAPTCHA, content-delivery, and security
        services and processes connection data for those purposes. Our hosting and database
        providers process and back up service data as needed to run TCGLense. Card data and prices
        come from Scryfall, TCGCSV, and MTGJSON. Card images are served through our proxy, so ordinary
        catalog browsing does not send your browser directly to those data providers. When you ask
        us to import or sync an external collection, we contact the selected provider using the
        collection identifier you supplied.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">5. What we never do</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        We do not sell personal data or show targeted ads. Your wish list is private. Collections
        and decks are private by default, but you may deliberately publish individual game
        collections or decks. Public profiles expose your chosen handle and account creation date;
        published views expose collection contents or deck names, descriptions, and contents. They
        are available to anyone, may be indexed by search engines, and are not limited to people you
        send the link to. Making an item private again removes origin access; previously cached
        copies can remain available briefly until shared caches expire.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">6. Retention and deletion</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        Account data, including an incomplete registration, is kept while that account record
        exists. Session and emailed tokens expire and are no longer accepted after their configured
        lifetime; limited records may remain until routine cleanup. API keys remain usable until
        their chosen expiry (if any), revocation, or account deletion. To have your account and its
        associated data deleted, email
        <a
          href="mailto:privacy@tcglense.com"
          class="text-primary underline-offset-4 hover:underline"
        >
          privacy@tcglense.com</a
        >
        from the account's email address. Copies in disaster-recovery backups may remain until those
        backups rotate out and are not used for ordinary product access.
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

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">9. Contact</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        Questions about this policy or your data: email
        <a
          href="mailto:privacy@tcglense.com"
          class="text-primary underline-offset-4 hover:underline"
        >
          privacy@tcglense.com</a
        >.
      </p>
    </section>
  </LegalPageLayout>
</template>
