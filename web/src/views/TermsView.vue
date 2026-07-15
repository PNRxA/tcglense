<script setup lang="ts">
import LegalPageLayout from '@/components/legal/LegalPageLayout.vue'
import { usePageMeta } from '@/lib/seo'

usePageMeta({
  title: 'Terms of Service',
  description:
    'The terms of service for TCGLense — a free, open-source trading-card price and ' +
    'collection tracker.',
  canonicalPath: '/terms',
})
</script>

<template>
  <LegalPageLayout
    title="Terms of Service"
    last-updated="July 15, 2026"
    sibling-label="Privacy Policy"
    sibling-to="/privacy"
  >
    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">1. What TCGLense is</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        TCGLense is a free service for browsing trading-card catalogs, following card and
        sealed-product prices over time, and tracking a personal collection and wish list. The
        software is open source; the code is available on GitHub.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">2. Your account</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        You register with an email address and finish setting up your account through a link we send
        you. Keep your password to yourself: everything done while signed in to your account is
        treated as done by you. You are responsible for the accuracy of the email address on the
        account.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">3. Acceptable use</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        Don't abuse the service: no attempting to access other users' data, no circumventing rate
        limits or CAPTCHA, no bulk-scraping the catalog or image endpoints, and no using the service
        to break the law. We may throttle, suspend, or remove accounts that do.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">4. Prices are informational</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        All prices shown are estimates derived from third-party data sources. They are provided for
        information only — they are not offers to buy or sell, not a valuation service, and not
        financial advice. We make no guarantee that any price is accurate, complete, or current.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">
        5. Third-party content and intellectual property
      </h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        Card names, card images, set names, and related materials belong to their respective
        publishers. TCGLense is unofficial Fan Content permitted under the Fan Content Policy. Not
        approved or endorsed by Wizards. Portions of the materials used are property of Wizards of
        the Coast. © Wizards of the Coast LLC. Catalog and price data come from Scryfall, TCGCSV,
        and MTGJSON, which are independent projects not affiliated with TCGLense. Card data and
        images are used courtesy of Scryfall.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">6. Your data</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        Your collection, wish-list, and deck entries remain yours. Imports from external services
        (such as Archidekt link imports, or CSV exports from Archidekt or Moxfield) are processed to
        fill your own collection. Collections and decks are private by default; if you choose to
        publish one, you can make it private again at any time. You are responsible for content you
        put in a public deck name or description.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">7. The service is provided as-is</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        TCGLense is provided without warranty of any kind. We may change, interrupt, or discontinue
        any part of the service at any time. To the maximum extent permitted by law, we are not
        liable for losses arising from your use of the service — including decisions made on the
        basis of price data.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">8. Changes to these terms</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        We may update these terms; the "Last updated" date above reflects the current version.
        Continuing to use the service after a change means you accept the updated terms.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-xl font-semibold tracking-tight">9. Contact</h2>
      <p class="text-muted-foreground leading-7 text-pretty">
        Questions, problems, or account requests: email
        <a
          href="mailto:contact@tcglense.com"
          class="text-primary underline-offset-4 hover:underline"
        >
          contact@tcglense.com</a
        >, or open an issue on the GitHub repository at
        <a
          href="https://github.com/PNRxA/tcglense"
          target="_blank"
          rel="noopener noreferrer"
          class="text-primary underline-offset-4 hover:underline"
        >
          github.com/PNRxA/tcglense</a
        >.
      </p>
    </section>
  </LegalPageLayout>
</template>
