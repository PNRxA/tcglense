<script setup lang="ts">
import RuleChip from '@/components/stack/RuleChip.vue'
import { PRIMER } from '@/lib/stack/primer'
import { RULES } from '@/lib/stack/rules'

// The reference half of the page: the rules the simulator applies, in reading order, each
// with its citation. Rendered from the same table the log cites, so the explanation a
// player reads here is word for word the one a log line opens.
</script>

<template>
  <section aria-labelledby="primer-heading">
    <h2 id="primer-heading" class="text-xl font-semibold tracking-tight">How the stack works</h2>
    <p class="text-muted-foreground mt-1 text-sm">
      The rules behind every line in the log, in the order they matter. Paraphrased from the
      Comprehensive Rules; the paragraph numbers lead to the authoritative text.
    </p>
    <div class="mt-4 grid gap-4 md:grid-cols-2">
      <article
        v-for="section in PRIMER"
        :key="section.title"
        class="bg-card rounded-xl border p-4 shadow-sm"
      >
        <h3 class="font-semibold">{{ section.title }}</h3>
        <p class="text-muted-foreground mt-1 text-sm leading-relaxed">{{ section.intro }}</p>
        <dl class="mt-3 space-y-3">
          <template v-for="id in section.rules" :key="id">
            <div v-if="RULES[id]">
              <dt class="flex flex-wrap items-center gap-2 text-sm font-medium">
                {{ RULES[id]!.title }}
                <RuleChip :rule="id" />
              </dt>
              <dd class="text-muted-foreground mt-0.5 text-sm leading-relaxed">
                {{ RULES[id]!.text }}
              </dd>
            </div>
          </template>
        </dl>
      </article>
    </div>
  </section>
</template>
