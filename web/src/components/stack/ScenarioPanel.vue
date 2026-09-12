<script setup lang="ts">
import { computed } from 'vue'
import { ChevronRight, GraduationCap, RotateCcw } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { SCENARIOS, type Scenario, type ScenarioStep } from '@/lib/stack/scenarios'

// The guided walkthroughs: pick one, read the lesson, step through it. Each step shows what
// to watch for *before* it runs, so the player predicts the outcome and then sees the log
// confirm it — the way a rules tutor at the table would teach it.
const props = defineProps<{
  scenario: Scenario | null
  stepIndex: number
  currentStep: ScenarioStep | null
  done: boolean
  /** The player acted off the script; undo rejoins it, restart replays it. */
  offScript: boolean
}>()
const emit = defineEmits<{ load: [slug: string | null]; next: []; restart: [] }>()

const FREE_PLAY = 'free-play'
const value = computed(() => props.scenario?.slug ?? FREE_PLAY)

function onChange(next: unknown) {
  if (typeof next !== 'string') return
  emit('load', next === FREE_PLAY ? null : next)
}
</script>

<template>
  <section class="bg-card rounded-xl border p-4 shadow-sm" aria-labelledby="walkthrough-heading">
    <div class="flex flex-wrap items-center gap-3">
      <h2 id="walkthrough-heading" class="flex items-center gap-2 text-sm font-semibold">
        <GraduationCap class="size-4" aria-hidden="true" /> Walkthrough
      </h2>
      <Select :model-value="value" @update:model-value="onChange">
        <SelectTrigger
          size="sm"
          class="min-w-56 flex-1 sm:flex-none"
          aria-label="Choose a walkthrough"
          data-testid="scenario-select"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem :value="FREE_PLAY">Free play — an empty table</SelectItem>
          <SelectItem v-for="option in SCENARIOS" :key="option.slug" :value="option.slug">
            {{ option.title }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <template v-if="scenario">
      <p class="mt-3 text-sm leading-relaxed">{{ scenario.lesson }}</p>
      <div class="bg-background mt-3 rounded-lg border p-3">
        <p class="text-muted-foreground text-[0.65rem] font-medium tracking-wide uppercase">
          {{
            offScript
              ? 'Off the script'
              : done
                ? 'Walkthrough complete'
                : `Step ${stepIndex + 1} of ${scenario.steps.length}`
          }}
        </p>
        <p v-if="offScript" class="mt-1 text-sm leading-relaxed" data-testid="off-script">
          You took an action of your own, so the table no longer matches the lesson. Undo to rejoin
          the script at step {{ stepIndex + 1 }}, or restart it.
        </p>
        <p v-else-if="currentStep" class="mt-1 text-sm leading-relaxed" data-testid="step-say">
          {{ currentStep.say }}
        </p>
        <p v-else class="mt-1 text-sm leading-relaxed">
          That's the whole lesson. Undo steps to re-read the log, restart to see it again, or keep
          playing on this table — any action of your own leaves the script.
        </p>
        <div class="mt-3 flex flex-wrap gap-2">
          <Button v-if="currentStep" size="sm" data-testid="next-step" @click="emit('next')">
            Next step <ChevronRight class="size-4" aria-hidden="true" />
          </Button>
          <Button size="sm" variant="outline" @click="emit('restart')">
            <RotateCcw class="size-4" aria-hidden="true" /> Restart
          </Button>
        </div>
      </div>
    </template>
    <p v-else class="text-muted-foreground mt-3 text-sm">
      Free play: cast anything for either player and watch the log explain each step. Pick a
      walkthrough above for a guided lesson.
    </p>
  </section>
</template>
