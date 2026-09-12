<script setup lang="ts">
import { computed, toRef } from 'vue'
import { Layers, Redo2, RotateCcw, Undo2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import PageBreadcrumbs from '@/components/PageBreadcrumbs.vue'
import ActionPanel from '@/components/stack/ActionPanel.vue'
import HintPanel from '@/components/stack/HintPanel.vue'
import PlayerBoard from '@/components/stack/PlayerBoard.vue'
import ScenarioPanel from '@/components/stack/ScenarioPanel.vue'
import StackColumn from '@/components/stack/StackColumn.vue'
import StackLog from '@/components/stack/StackLog.vue'
import StackPrimer from '@/components/stack/StackPrimer.vue'
import { useGameName } from '@/composables/useCatalog'
import { useStackSimulator } from '@/composables/useStackSimulator'
import { usePageMeta } from '@/lib/seo'
import { stackPath, toolsFor, toolsPath } from '@/lib/tools'

// The stack simulator: a two-seat table where one person plays both sides and the engine
// narrates every rule it applies. Entirely client-side — there is nothing per-user to keep,
// so the page is public and indexable like the glossary, and a reload is a fresh table.
//
// Layout: the table (opponent's board, the stack, your board) on the left, the "what happens
// next" hint, the action panel and the log on the right; the walkthrough picker above them
// all and the rules primer below. On a phone the columns stack in that same reading order.
const props = defineProps<{ game: string }>()
const game = toRef(props, 'game')
const gameName = useGameName(game)
const available = computed(() => toolsFor(game.value).some((tool) => tool.slug === 'stack'))

const sim = useStackSimulator()

const crumbs = computed(() => [
  { label: 'Home', to: '/' },
  { label: 'Tools', to: '/tools' },
  { label: gameName.value, to: toolsPath(game.value) },
  { label: 'Stack simulator' },
])

usePageMeta({
  title: () => `${gameName.value} stack simulator`,
  description: () =>
    `See how the ${gameName.value} stack works: cast spells for both players, respond, pass ` +
    'priority, and read why each spell, ability and trigger resolves in the order it does — ' +
    'with the rule behind every step.',
  canonicalPath: () => stackPath(game.value),
})

const turnLabel = computed(() => {
  const active = sim.state.value.players[sim.state.value.activePlayer].name
  return `Turn ${sim.state.value.turn} · ${active === 'You' ? 'Your' : `${active}'s`} turn`
})
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-8">
    <PageBreadcrumbs :items="crumbs" />

    <header class="mb-6 flex flex-wrap items-start justify-between gap-4">
      <div>
        <h1 class="flex items-center gap-2 text-3xl font-semibold tracking-tight">
          <Layers class="size-7" aria-hidden="true" />
          Stack simulator
        </h1>
        <p class="text-muted-foreground mt-2 max-w-2xl">
          Cast spells for both players, respond, pass priority, and watch the stack resolve — with
          the reason for every step and the rule it comes from. Play both seats yourself.
        </p>
      </div>
      <div v-if="available" class="flex flex-wrap items-center gap-2">
        <span class="text-muted-foreground text-sm tabular-nums" data-testid="turn-label">{{
          turnLabel
        }}</span>
        <Button
          size="sm"
          variant="outline"
          :disabled="!sim.canUndo.value"
          data-testid="undo"
          @click="sim.undo"
        >
          <Undo2 class="size-4" aria-hidden="true" /> Undo
        </Button>
        <Button
          size="sm"
          variant="outline"
          :disabled="!sim.canRedo.value"
          data-testid="redo"
          @click="sim.redo"
        >
          <Redo2 class="size-4" aria-hidden="true" /> Redo
        </Button>
        <Button size="sm" variant="outline" data-testid="reset" @click="sim.reset">
          <RotateCcw class="size-4" aria-hidden="true" /> Reset table
        </Button>
      </div>
    </header>

    <p v-if="!available" class="text-muted-foreground py-12">
      There is no stack simulator for {{ gameName }} — it models the Magic: The Gathering rules.
    </p>

    <template v-else>
      <ScenarioPanel
        class="mb-6"
        :scenario="sim.scenario.value"
        :step-index="sim.stepIndex.value"
        :current-step="sim.currentStep.value"
        :done="sim.walkthroughDone.value"
        :off-script="sim.offScript.value"
        @load="sim.loadScenario"
        @next="sim.nextStep"
        @restart="sim.reset"
      />

      <div class="grid gap-6 lg:grid-cols-5">
        <div class="space-y-4 lg:col-span-3">
          <PlayerBoard :state="sim.state.value" player="opp" />
          <StackColumn :state="sim.state.value" />
          <PlayerBoard :state="sim.state.value" player="you" />
        </div>
        <div class="space-y-4 lg:col-span-2">
          <HintPanel :hint="sim.hint.value" :refusal="sim.refusal.value" />
          <ActionPanel :state="sim.state.value" @dispatch="sim.dispatch" />
          <StackLog :entries="sim.state.value.log" :latest="sim.latestEntries.value" />
        </div>
      </div>

      <StackPrimer class="mt-12" />
    </template>
  </div>
</template>
