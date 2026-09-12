import { computed, ref, shallowRef } from 'vue'
import { applyAction, lastRefusal } from '@/lib/stack/engine'
import { nextHint } from '@/lib/stack/hints'
import { scenarioBySlug, type Scenario } from '@/lib/stack/scenarios'
import { createState } from '@/lib/stack/state'
import type { Action, LogEntry, StackState } from '@/lib/stack/types'

/**
 * The simulator's session: an undo history of engine states, plus the guided-walkthrough
 * cursor. The engine is pure (`applyAction` returns a new state), so undo is just dropping
 * the last state — no reverse rules, and a walkthrough step is literally the same dispatch
 * a free action is, only chosen by the script rather than the player.
 *
 * Every entry remembers whether the script chose it: undoing a scripted step rewinds the
 * cursor, undoing a free one doesn't, and taking a free action mid-walkthrough leaves the
 * script (the story it tells no longer matches the table).
 */
interface HistoryEntry {
  state: StackState
  scripted: boolean
}

export function useStackSimulator() {
  const history = shallowRef<HistoryEntry[]>([{ state: createState(), scripted: false }])
  const future = shallowRef<HistoryEntry[]>([])
  const scenario = ref<Scenario | null>(null)
  const stepIndex = ref(0)

  const state = computed(() => history.value[history.value.length - 1]!.state)
  const previous = computed(() => history.value[history.value.length - 2]?.state ?? null)
  /** The log lines the most recent action added — what the log highlights. */
  const latestEntries = computed<LogEntry[]>(() => {
    const from = previous.value?.log.length ?? 0
    return state.value.log.slice(from)
  })
  const refusal = computed(() => lastRefusal(state.value))
  const hint = computed(() => nextHint(state.value))
  const canUndo = computed(() => history.value.length > 1)
  const canRedo = computed(() => future.value.length > 0)

  const currentStep = computed(() => scenario.value?.steps[stepIndex.value] ?? null)
  const walkthroughDone = computed(
    () => !!scenario.value && stepIndex.value >= scenario.value.steps.length,
  )

  function push(entry: HistoryEntry) {
    history.value = [...history.value, entry]
    future.value = []
  }

  /** A player's own action. Leaves the walkthrough if one was running. */
  function dispatch(action: Action) {
    push({ state: applyAction(state.value, action), scripted: false })
    if (scenario.value) {
      scenario.value = null
      stepIndex.value = 0
    }
  }

  function nextStep() {
    const step = currentStep.value
    if (!step) return
    push({ state: applyAction(state.value, step.act(state.value)), scripted: true })
    stepIndex.value += 1
  }

  function undo() {
    if (!canUndo.value) return
    const entries = [...history.value]
    const popped = entries.pop()!
    history.value = entries
    future.value = [popped, ...future.value]
    if (popped.scripted && scenario.value) stepIndex.value = Math.max(0, stepIndex.value - 1)
  }

  function redo() {
    const [next, ...rest] = future.value
    if (!next) return
    history.value = [...history.value, next]
    future.value = rest
    if (next.scripted && scenario.value) stepIndex.value += 1
  }

  /** Start over on the same table: the walkthrough from its first step, or a clean board. */
  function reset() {
    history.value = [
      { state: scenario.value ? scenario.value.setup() : createState(), scripted: false },
    ]
    future.value = []
    stepIndex.value = 0
  }

  /** Load a walkthrough by slug, or `null` for free play on a clean table. */
  function loadScenario(slug: string | null) {
    scenario.value = slug ? (scenarioBySlug(slug) ?? null) : null
    reset()
  }

  return {
    state,
    latestEntries,
    refusal,
    hint,
    canUndo,
    canRedo,
    scenario,
    stepIndex,
    currentStep,
    walkthroughDone,
    dispatch,
    nextStep,
    undo,
    redo,
    reset,
    loadScenario,
  }
}
