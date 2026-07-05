import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import CardPagination from '../CardPagination.vue'

// The shared prev/next pager: it hides on a single page, clamps at the ends, and — issue #223
// — swaps both buttons' chevrons for a spinner and disables them while the next page loads,
// then restores them once the load resolves.
function mountPagination(
  props: { page?: number; pageSize?: number; total?: number; loading?: boolean } = {},
) {
  return mount(CardPagination, {
    props: {
      page: props.page ?? 1,
      pageSize: props.pageSize ?? 60,
      total: props.total ?? 300, // 5 pages by default
      ...(props.loading !== undefined ? { loading: props.loading } : {}),
    },
  })
}

const prev = (wrapper: ReturnType<typeof mountPagination>) => wrapper.findAll('button')[0]!
const next = (wrapper: ReturnType<typeof mountPagination>) => wrapper.findAll('button')[1]!

describe('CardPagination', () => {
  it('renders nothing when there is only one page', () => {
    const wrapper = mountPagination({ total: 40, pageSize: 60 })
    expect(wrapper.find('button').exists()).toBe(false)
  })

  it('emits the previous page when Prev is clicked', async () => {
    const wrapper = mountPagination({ page: 3 })
    await prev(wrapper).trigger('click')
    expect(wrapper.emitted('update:page')).toEqual([[2]])
  })

  it('emits the next page when Next is clicked', async () => {
    const wrapper = mountPagination({ page: 3 })
    await next(wrapper).trigger('click')
    expect(wrapper.emitted('update:page')).toEqual([[4]])
  })

  it('disables Prev on the first page and Next on the last', () => {
    const first = mountPagination({ page: 1 })
    expect(prev(first).attributes('disabled')).toBeDefined()
    expect(next(first).attributes('disabled')).toBeUndefined()
    const last = mountPagination({ page: 5 })
    expect(prev(last).attributes('disabled')).toBeUndefined()
    expect(next(last).attributes('disabled')).toBeDefined()
  })

  it('spins and disables both buttons while a page loads, then restores them (#223)', async () => {
    const wrapper = mountPagination({ page: 3 })
    // Not loading → chevrons, no spinner.
    expect(wrapper.find('.animate-spin').exists()).toBe(false)
    // Loading → both buttons show a spinner and disable (so a page can't be double-requested).
    await wrapper.setProps({ loading: true })
    expect(prev(wrapper).find('.animate-spin').exists()).toBe(true)
    expect(next(wrapper).find('.animate-spin').exists()).toBe(true)
    expect(prev(wrapper).attributes('disabled')).toBeDefined()
    expect(next(wrapper).attributes('disabled')).toBeDefined()
    // Resolved → spinner clears.
    await wrapper.setProps({ loading: false })
    expect(wrapper.find('.animate-spin').exists()).toBe(false)
  })
})
