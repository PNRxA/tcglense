<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { Play, SkipForward } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { CARD_TYPE_LABELS, CARD_TYPE_ORDER, CARDS, cardById } from '@/lib/stack/cards'
import { activatability, castability, type Refusal } from '@/lib/stack/engine'
import {
  describeTarget,
  preferredTargets,
  sameTarget,
  TARGET_SPEC_LABELS,
} from '@/lib/stack/targets'
import type { Action, StackState, TargetRef, TargetSpec } from '@/lib/stack/types'

// What the player with priority can do right now: pass, cast any card in the library, or
// activate one of their permanents' abilities — each option showing *why* it is off when it
// is, since the refusals are the lesson. Both players are driven from here (the simulator
// is one person playing both sides), so the panel follows priority rather than a fixed seat.
const props = defineProps<{ state: StackState }>()
const emit = defineEmits<{ dispatch: [action: Action] }>()

const actor = computed(() => props.state.priority)
const actorName = computed(() => props.state.players[actor.value].name)
const gameOver = computed(() => props.state.loser !== null)

type Selection = { kind: 'card'; id: string } | { kind: 'permanent'; id: number } | null
const selection = ref<Selection>(null)
const target = ref<TargetRef | null>(null)
const formEl = ref<HTMLElement | null>(null)

/** The selection, only while what it points at still exists — a permanent can leave the
 * battlefield under an undo, a reset or a loaded walkthrough without priority changing. */
const resolved = computed<Selection>(() => {
  const chosen = selection.value
  if (!chosen) return null
  if (chosen.kind === 'card') return cardById(chosen.id) ? chosen : null
  return props.state.battlefield.some((p) => p.id === chosen.id) ? chosen : null
})

const cardOptions = computed(() =>
  CARD_TYPE_ORDER.map((type) => ({
    type,
    label: CARD_TYPE_LABELS[type],
    cards: CARDS.filter((card) => card.type === type).map((card) => ({
      card,
      allowed: castability(props.state, actor.value, card),
    })),
  })),
)

const abilityOptions = computed(() =>
  props.state.battlefield
    .filter((p) => p.controller === actor.value && cardById(p.cardId)?.activated)
    .map((permanent) => ({
      permanent,
      ability: cardById(permanent.cardId)!.activated!,
      allowed: activatability(props.state, actor.value, permanent),
    })),
)

/** The chosen card or ability's target requirement and what it can aim at right now. */
const targeting = computed<{ spec: TargetSpec; refs: TargetRef[] } | null>(() => {
  const chosen = selection.value
  if (!chosen) return null
  if (chosen.kind === 'card') {
    const card = cardById(chosen.id)
    if (!card?.target) return null
    return {
      spec: card.target,
      refs: preferredTargets(props.state, card.target, actor.value, card.effect),
    }
  }
  const permanent = props.state.battlefield.find((p) => p.id === chosen.id)
  const ability = permanent ? cardById(permanent.cardId)?.activated : undefined
  if (!ability?.target) return null
  return {
    spec: ability.target,
    refs: preferredTargets(props.state, ability.target, actor.value, ability.effect),
  }
})

const selectedAllowed = computed<Refusal | null>(() => {
  const chosen = selection.value
  if (!chosen) return null
  if (chosen.kind === 'card') {
    const card = cardById(chosen.id)
    return card ? castability(props.state, actor.value, card) : null
  }
  const permanent = props.state.battlefield.find((p) => p.id === chosen.id)
  return permanent ? activatability(props.state, actor.value, permanent) : null
})

const selectedText = computed(() => {
  const chosen = selection.value
  if (!chosen) return ''
  if (chosen.kind === 'card') return cardById(chosen.id)?.text ?? ''
  const permanent = props.state.battlefield.find((p) => p.id === chosen.id)
  return permanent ? (cardById(permanent.cardId)?.activated?.text ?? '') : ''
})

const selectedName = computed(() => {
  const chosen = selection.value
  if (!chosen) return ''
  if (chosen.kind === 'card') return cardById(chosen.id)?.name ?? ''
  const permanent = props.state.battlefield.find((p) => p.id === chosen.id)
  return permanent ? `${permanent.name}'s ability` : ''
})

// A new selection (or a table change under the same one) re-picks the most plausible target;
// a target the player chose stays only while it is still legal.
watch(
  [selection, targeting],
  () => {
    const refs = targeting.value?.refs ?? []
    const current = target.value
    if (current && refs.some((ref) => sameTarget(ref, current))) return
    target.value = refs[0] ?? null
  },
  { immediate: true },
)

// Priority moving to the other player means a different hand of options — start clean.
watch(actor, () => {
  selection.value = null
})

// The detail box sits below the whole library, off-screen on a phone: bring it into view and
// give it focus so a keyboard or screen-reader user isn't left tabbing through the rest.
watch(resolved, (next) => {
  if (!next) return
  nextTick(() => {
    const el = formEl.value
    if (!el) return
    if (typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'nearest' })
    el.focus({ preventScroll: true })
  })
})

const targetKey = (ref: TargetRef) => `${ref.kind}:${ref.id}`
const targetValue = computed(() => (target.value ? targetKey(target.value) : ''))
function onTargetChange(next: unknown) {
  target.value = targeting.value?.refs.find((ref) => targetKey(ref) === next) ?? null
}

const canGo = computed(
  () => !!resolved.value && !!selectedAllowed.value?.ok && (!targeting.value || !!target.value),
)

function go() {
  const chosen = resolved.value
  if (!chosen || !canGo.value) return
  const chosenTarget = target.value ?? undefined
  if (chosen.kind === 'card') {
    emit('dispatch', {
      type: 'cast',
      player: actor.value,
      cardId: chosen.id,
      ...(chosenTarget ? { target: chosenTarget } : {}),
    })
  } else {
    emit('dispatch', {
      type: 'activate',
      player: actor.value,
      permanentId: chosen.id,
      ...(chosenTarget ? { target: chosenTarget } : {}),
    })
  }
  selection.value = null
}

function pass() {
  emit('dispatch', { type: 'pass', player: actor.value })
}

function isSelected(candidate: Selection): boolean {
  return (
    !!selection.value &&
    !!candidate &&
    selection.value.kind === candidate.kind &&
    selection.value.id === candidate.id
  )
}

const optionClass = (selected: boolean, allowed: boolean) =>
  [
    'w-full rounded-lg border px-3 py-1.5 text-left text-sm transition-colors focus-visible:ring-ring/50 focus-visible:ring-2 focus-visible:outline-none',
    selected ? 'border-primary bg-primary/10' : 'bg-background hover:bg-accent/40',
    allowed ? '' : 'opacity-60',
  ].join(' ')
</script>

<template>
  <section class="bg-card rounded-xl border p-4 shadow-sm" aria-labelledby="actions-heading">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h2 id="actions-heading" class="text-sm font-semibold">
        {{ gameOver ? 'Game over' : `${actorName} ${actor === 'you' ? 'have' : 'has'} priority` }}
      </h2>
      <Button
        size="sm"
        variant="outline"
        :disabled="gameOver"
        data-testid="pass-priority"
        @click="pass"
      >
        <SkipForward class="size-4" aria-hidden="true" /> Pass priority
      </Button>
    </div>
    <p class="text-muted-foreground mt-1 text-xs">
      Pick a card to cast or an ability to activate, then choose its target. Mana is never charged —
      the tool is about order, not cost.
    </p>

    <div class="mt-3 space-y-3">
      <div v-if="abilityOptions.length">
        <p class="text-muted-foreground mb-1 text-[0.65rem] font-medium tracking-wide uppercase">
          Abilities
        </p>
        <ul class="space-y-1">
          <li v-for="row in abilityOptions" :key="row.permanent.id">
            <button
              type="button"
              :class="
                optionClass(isSelected({ kind: 'permanent', id: row.permanent.id }), row.allowed.ok)
              "
              :aria-pressed="isSelected({ kind: 'permanent', id: row.permanent.id })"
              @click="selection = { kind: 'permanent', id: row.permanent.id }"
            >
              <span class="font-medium">{{ row.permanent.name }}</span>
              <span class="text-muted-foreground"> — {{ row.ability.text }}</span>
              <span
                v-if="row.ability.mana"
                class="bg-muted text-muted-foreground ml-1 rounded-full px-1.5 py-0.5 text-[0.65rem]"
              >
                mana ability
              </span>
            </button>
          </li>
        </ul>
      </div>

      <div v-for="group in cardOptions" :key="group.type">
        <p class="text-muted-foreground mb-1 text-[0.65rem] font-medium tracking-wide uppercase">
          {{ group.label }}
        </p>
        <ul class="grid gap-1 sm:grid-cols-2">
          <li v-for="row in group.cards" :key="row.card.id">
            <button
              type="button"
              :class="optionClass(isSelected({ kind: 'card', id: row.card.id }), row.allowed.ok)"
              :aria-pressed="isSelected({ kind: 'card', id: row.card.id })"
              :data-testid="`cast-${row.card.id}`"
              :title="row.card.text || undefined"
              @click="selection = { kind: 'card', id: row.card.id }"
            >
              <span class="flex items-center gap-2">
                <span class="truncate font-medium">{{ row.card.name }}</span>
                <span class="text-muted-foreground ml-auto shrink-0 text-xs tabular-nums">
                  {{ row.card.manaCost }}
                </span>
              </span>
            </button>
          </li>
        </ul>
      </div>
    </div>

    <div
      v-if="resolved"
      ref="formEl"
      class="bg-background focus-visible:ring-ring/50 mt-3 rounded-lg border p-3 focus-visible:ring-2 focus-visible:outline-none"
      tabindex="-1"
      data-testid="cast-form"
    >
      <p class="text-sm font-medium">{{ selectedName }}</p>
      <p v-if="selectedText" class="text-muted-foreground mt-0.5 text-xs leading-snug">
        {{ selectedText }}
      </p>
      <p
        v-if="selectedAllowed && !selectedAllowed.ok"
        class="text-destructive mt-1 text-xs"
        data-testid="cast-refusal"
      >
        {{ selectedAllowed.reason }}
      </p>
      <template v-else-if="targeting">
        <label class="text-muted-foreground mt-2 block text-xs" for="stack-target">
          Target — {{ TARGET_SPEC_LABELS[targeting.spec.kind] }}
        </label>
        <Select :model-value="targetValue" @update:model-value="onTargetChange">
          <SelectTrigger
            id="stack-target"
            size="sm"
            class="mt-1 w-full"
            data-testid="target-select"
          >
            <SelectValue placeholder="Choose a target" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="ref in targeting.refs" :key="targetKey(ref)" :value="targetKey(ref)">
              {{ describeTarget(state, ref) }}
            </SelectItem>
          </SelectContent>
        </Select>
      </template>
      <div class="mt-3 flex gap-2">
        <Button size="sm" :disabled="!canGo" data-testid="confirm-action" @click="go">
          <Play class="size-4" aria-hidden="true" />
          {{ resolved.kind === 'card' ? 'Cast' : 'Activate' }}
        </Button>
        <Button size="sm" variant="ghost" @click="selection = null">Cancel</Button>
      </div>
    </div>
  </section>
</template>
