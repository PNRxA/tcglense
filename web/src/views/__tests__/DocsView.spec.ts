import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia } from 'pinia'

vi.mock('@scalar/api-reference', () => ({
  ApiReference: {
    template: '<div data-testid="api-reference" />',
  },
}))

import DocsView from '@/views/DocsView.vue'
import { onMaintenanceDetected } from '@/lib/maintenance'

function fakeResponse(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    text: () => Promise.resolve(JSON.stringify(body)),
  } as Response
}

const fetchMock = vi.fn<typeof fetch>()

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock)
  fetchMock.mockReset()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('DocsView', () => {
  it('renders Scalar after loading the OpenAPI document through the API client', async () => {
    fetchMock.mockResolvedValueOnce(fakeResponse(200, { openapi: '3.1.0' }))
    const wrapper = mount(DocsView, { global: { plugins: [createPinia()] } })

    await flushPromises()

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/openapi.json',
      expect.objectContaining({ credentials: 'include' }),
    )
    expect(wrapper.find('[data-testid="api-reference"]').exists()).toBe(true)
    wrapper.unmount()
  })

  it('signals a coded maintenance response instead of remaining silently stuck', async () => {
    const detected = vi.fn<() => void>()
    const unsubscribe = onMaintenanceDetected(detected)
    fetchMock.mockResolvedValueOnce(
      fakeResponse(503, {
        error: 'service is under maintenance',
        code: 'maintenance',
      }),
    )
    const wrapper = mount(DocsView, { global: { plugins: [createPinia()] } })

    await flushPromises()

    expect(detected).toHaveBeenCalledOnce()
    expect(wrapper.text()).toContain('Loading API reference')
    unsubscribe()
    wrapper.unmount()
  })
})
