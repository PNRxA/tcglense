import { defineComponent, nextTick, type Ref } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { describe, expect, it } from 'vitest'
import { useSecretToken } from '../useSecretToken'

function makeRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/complete-registration', component: { template: '<div />' } }],
  })
}

describe('useSecretToken', () => {
  it('captures the token in memory and scrubs only that query value', async () => {
    const router = makeRouter()
    await router.push('/complete-registration?token=secret&redirect=/collection/mtg#step')

    let captured!: Ref<string | null>
    const wrapper = mount(
      defineComponent({
        setup() {
          captured = useSecretToken()
          return () => null
        },
      }),
      { global: { plugins: [router] } },
    )
    await flushPromises()

    expect(captured.value).toBe('secret')
    expect(router.currentRoute.value.query).toEqual({ redirect: '/collection/mtg' })
    expect(router.currentRoute.value.hash).toBe('#step')
    wrapper.unmount()
  })

  it('captures and scrubs a fresh link opened into an already-mounted view', async () => {
    const router = makeRouter()
    await router.push('/complete-registration')

    let captured!: Ref<string | null>
    const wrapper = mount(
      defineComponent({
        setup() {
          captured = useSecretToken()
          return () => null
        },
      }),
      { global: { plugins: [router] } },
    )

    await router.push('/complete-registration?token=next&redirect=/decks/mtg')
    await nextTick()
    await flushPromises()

    expect(captured.value).toBe('next')
    expect(router.currentRoute.value.query).toEqual({ redirect: '/decks/mtg' })
    wrapper.unmount()
  })

  it('does not retain an older credential when a new token query is malformed', async () => {
    const router = makeRouter()
    await router.push('/complete-registration?token=first')

    let captured!: Ref<string | null>
    const wrapper = mount(
      defineComponent({
        setup() {
          captured = useSecretToken()
          return () => null
        },
      }),
      { global: { plugins: [router] } },
    )
    await flushPromises()
    expect(captured.value).toBe('first')

    await router.push({ path: '/complete-registration', query: { token: ['second', 'third'] } })
    await flushPromises()

    expect(captured.value).toBeNull()
    expect(router.currentRoute.value.query).toEqual({})
    wrapper.unmount()
  })
})
