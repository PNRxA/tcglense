import { describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
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

const Stub = defineComponent({ template: '<div />' })

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
  const wrapper = mount(StackSimulatorView, { props: { game }, global: { plugins: [router] } })
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
    expect(wrapper.text()).toContain('17')
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

  it('says so for a game without the tool', async () => {
    const wrapper = await mountView('pkm')
    expect(wrapper.text()).toContain('There is no stack simulator for Magic')
    expect(wrapper.find('[data-testid="pass-priority"]').exists()).toBe(false)
  })
})
