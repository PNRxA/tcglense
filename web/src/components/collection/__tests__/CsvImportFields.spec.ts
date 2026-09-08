import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import CsvImportFields from '@/components/collection/CsvImportFields.vue'

// The upload tab's file-format help. The server sniffs the shape, so the user never picks
// a provider — which makes this hint the only place the dialog says which exports work.
// Every supported CSV export must be named here, or a user holding one has no way to
// know it's accepted (issue #669: ManaBox is the default phone scanner, so it's the most
// common file a new user arrives with).

describe('CsvImportFields', () => {
  it('names every supported export in the format help', () => {
    const wrapper = mount(CsvImportFields)
    const text = wrapper.text()
    for (const provider of ['ManaBox', 'Archidekt', 'Moxfield', 'Mythic Tools']) {
      expect(text).toContain(`Exporting from ${provider}`)
    }
  })

  it('emits the picked file, and null when the picker is cleared', async () => {
    const wrapper = mount(CsvImportFields)
    const input = wrapper.get('input[type="file"]')
    const file = new File(['Name,ManaBox ID\n'], 'ManaBox_Collection.csv', { type: 'text/csv' })
    Object.defineProperty(input.element, 'files', { value: [file], configurable: true })
    await input.trigger('change')
    expect(wrapper.emitted('fileChange')?.[0]).toEqual([file])

    Object.defineProperty(input.element, 'files', { value: [], configurable: true })
    await input.trigger('change')
    expect(wrapper.emitted('fileChange')?.[1]).toEqual([null])
  })
})
