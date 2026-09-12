import { describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'

// The page's job is wiring the engine to the panels: an action taken through the action panel
// lands on the stack, passing resolves it, the log narrates and the hint follows. The engine's
// rules are pinned in lib/__tests__/stack.spec; the game name is mocked at the catalog seam.
vi.mock('@/composables/useCatalog', async () => {
  const { ref: vueRef } = await import('vue')
  return {
    useGamesQuery: () => ({ data: vueRef({ data: [{ id: 'mtg', name: 'Magic' }] }) }),
    useGameName: () => vueRef('Magic'),
  }
})

vi.mock('@/lib/seo', () => ({ usePageMeta: vi.fn<() => void>() }))

import StackSimulatorView from '@/views/StackSimulatorView.vue'
import { SCENARIOS } from '@/lib/stack/scenarios'

const Stub = defineComponent({ template: '<div />' })

// A plain `<select>` in place of the shadcn Select (the DeckCompare spec's idiom), so a pick
// in the walkthrough picker or the target picker is one DOM event.
const SelectStub = defineComponent({
  props: { modelValue: { type: String, required: true } },
  emits: ['update:modelValue'],
  setup(props, { emit, slots }) {
    return () =>
      h(
        'select',
        {
          value: props.modelValue,
          onChange: (e: Event) => emit('update:modelValue', (e.target as HTMLSelectElement).value),
        },
        slots.default?.(),
      )
  },
})
const SelectItemStub = defineComponent({
  props: { value: { type: String, required: true } },
  setup(props, { slots }) {
    return () => h('option', { value: props.value }, slots.default?.())
  },
})
const Passthrough = defineComponent({
  setup(_, { slots }) {
    return () => slots.default?.()
  },
})

async function mountView(game = 'mtg') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: Stub },
      { path: '/tools', component: Stub },
      { path: '/tools/:game', component: Stub },
      { path: '/tools/:game/stack', component: StackSimulatorView, props: true },
    ],
  })
  await router.push(`/tools/${game}/stack`)
  await router.isReady()
  const wrapper = mount(StackSimulatorView, {
    props: { game },
    global: {
      plugins: [router],
      stubs: {
        Select: SelectStub,
        SelectItem: SelectItemStub,
        SelectTrigger: Passthrough,
        SelectContent: Passthrough,
        SelectValue: Passthrough,
      },
    },
  })
  await flushPromises()
  return wrapper
}

describe('StackSimulatorView', () => {
  it('casts through the action panel, then resolves after both players pass', async () => {
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="hint"]').text()).toMatch(
      /stack is empty and you have priority/,
    )

    await wrapper.find('[data-testid="cast-lightning-bolt"]').trigger('click')
    const form = wrapper.find('[data-testid="cast-form"]')
    expect(form.exists()).toBe(true)
    await wrapper.find('[data-testid="confirm-action"]').trigger('click')
    await flushPromises()

    const stack = wrapper.find('[data-testid="stack-objects"]')
    expect(stack.text()).toContain('Lightning Bolt')
    expect(stack.text()).toContain('targeting Opponent')
    expect(stack.text()).toContain('Resolves first')
    expect(wrapper.find('[data-testid="stack-log"]').text()).toMatch(
      /You cast Lightning Bolt targeting Opponent/,
    )
    expect(wrapper.find('[data-testid="hint"]').text()).toMatch(
      /Lightning Bolt is on top of the stack/,
    )

    await wrapper.find('[data-testid="pass-priority"]').trigger('click')
    await wrapper.find('[data-testid="pass-priority"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="stack-objects"]').exists()).toBe(false)
    const board = (name: string) => wrapper.get(`section[aria-label="${name}'s board"]`)
    expect(board('Opponent').text()).toContain('17')
    expect(board('You').text()).toContain('20')
    expect(wrapper.find('[data-testid="stack-log"]').text()).toMatch(
      /Lightning Bolt deals 3 damage to Opponent/,
    )
    expect(wrapper.find('[data-testid="undo"]').attributes('disabled')).toBeUndefined()
  })

  it('greys a sorcery out with the engine’s reason while the stack is not empty', async () => {
    const wrapper = await mountView()
    await wrapper.find('[data-testid="cast-lightning-bolt"]').trigger('click')
    await wrapper.find('[data-testid="confirm-action"]').trigger('click')
    await wrapper.find('[data-testid="cast-divination"]').trigger('click')
    expect(wrapper.find('[data-testid="cast-refusal"]').text()).toMatch(/stack is empty/)
    expect(wrapper.find('[data-testid="confirm-action"]').attributes('disabled')).toBeDefined()
  })

  it('undoes the last action', async () => {
    const wrapper = await mountView()
    await wrapper.find('[data-testid="cast-lightning-bolt"]').trigger('click')
    await wrapper.find('[data-testid="confirm-action"]').trigger('click')
    await wrapper.find('[data-testid="undo"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="stack-objects"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="redo"]').attributes('disabled')).toBeUndefined()
  })

  it('runs a walkthrough from the picker and shows the refusal it narrates', async () => {
    const wrapper = await mountView()
    const scenario = SCENARIOS.find((s) => s.slug === 'sorcery-timing')!
    await wrapper.get('[data-testid="scenario-select"] select, select').setValue('sorcery-timing')
    await flushPromises()
    expect(wrapper.find('[data-testid="step-say"]').text()).toBe(scenario.steps[0]!.say)
    await wrapper.find('[data-testid="next-step"]').trigger('click')
    await wrapper.find('[data-testid="next-step"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="stack-objects"]').text()).toContain('Divination')
    // Step two narrates a refusal (Wrath of God with a spell on the stack): the hint shows it.
    expect(wrapper.find('[data-testid="refusal"]').text()).toMatch(/Wrath of God is a sorcery/)
    expect(wrapper.find('[data-testid="step-say"]').text()).toBe(scenario.steps[2]!.say)
    // A free action leaves the script; undo rejoins it.
    await wrapper.find('[data-testid="pass-priority"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="off-script"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="next-step"]').exists()).toBe(false)
    await wrapper.find('[data-testid="undo"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="off-script"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="step-say"]').text()).toBe(scenario.steps[2]!.say)
  })

  it('activates an ability through the target picker', async () => {
    const wrapper = await mountView()
    // Put a Prodigal Sorcerer on your side by casting it and letting it resolve, then move
    // two turns on so it is no longer summoning sick.
    await wrapper.find('[data-testid="cast-prodigal-sorcerer"]').trigger('click')
    await wrapper.find('[data-testid="confirm-action"]').trigger('click')
    for (let i = 0; i < 6; i++) await wrapper.find('[data-testid="pass-priority"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="turn-label"]').text()).toContain('Turn 3')
    const ability = wrapper.findAll('button').find((b) => b.text().includes('Prodigal Sorcerer'))!
    await ability.trigger('click')
    await flushPromises()
    const picker = wrapper.get('[data-testid="cast-form"] select')
    // Harm aims at the opponent first, then your own things — the Sorcerer itself included.
    expect(picker.findAll('option').map((o) => o.text())).toEqual([
      'Opponent',
      'Prodigal Sorcerer (yours)',
      'You',
    ])
    await picker.setValue('player:you')
    await wrapper.find('[data-testid="confirm-action"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="stack-objects"]').text()).toContain('targeting You')
  })

  it('says so for a game without the tool', async () => {
    const wrapper = await mountView('pkm')
    expect(wrapper.text()).toContain('There is no stack simulator for Magic')
    expect(wrapper.find('[data-testid="pass-priority"]').exists()).toBe(false)
  })
})
