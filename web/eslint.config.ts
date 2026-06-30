import { globalIgnores } from 'eslint/config'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'
import pluginVue from 'eslint-plugin-vue'
import pluginPlaywright from 'eslint-plugin-playwright'
import pluginVitest from '@vitest/eslint-plugin'
import pluginOxlint from 'eslint-plugin-oxlint'
import skipFormatting from 'eslint-config-prettier/flat'

// To allow more languages other than `ts` in `.vue` files, uncomment the following lines:
// import { configureVueProject } from '@vue/eslint-config-typescript'
// configureVueProject({ scriptLangs: ['ts', 'tsx'] })
// More info at https://github.com/vuejs/eslint-config-typescript/#advanced-setup

export default defineConfigWithVueTs(
  {
    name: 'app/files-to-lint',
    files: ['**/*.{vue,ts,mts,tsx}'],
  },

  globalIgnores(['**/dist/**', '**/dist-ssr/**', '**/coverage/**']),

  ...pluginVue.configs['flat/essential'],
  vueTsConfigs.recommended,

  {
    ...pluginPlaywright.configs['flat/recommended'],
    files: ['e2e/**/*.{test,spec}.{js,ts,jsx,tsx}'],
  },

  {
    ...pluginVitest.configs.recommended,
    files: ['src/**/__tests__/*'],
  },

  {
    name: 'app/ui-components',
    files: ['src/components/ui/**/*.{vue,ts}'],
    rules: {
      // shadcn-vue primitives are intentionally single-word (Button, Card, Input, ...).
      'vue/multi-word-component-names': 'off',
      // Primitives strip `class` (and `viewport`) via `const { class: _class, ...rest } = props`
      // before forwarding the rest to the underlying reka-ui component — the discarded
      // siblings are deliberate, not dead code.
      '@typescript-eslint/no-unused-vars': ['error', { ignoreRestSiblings: true }],
      // The shadcn-vue chart primitives (Unovis crosshair render bridge) ship `any`
      // in their generated payload/accessor plumbing — vendored code we keep verbatim.
      '@typescript-eslint/no-explicit-any': 'off',
    },
  },

  ...pluginOxlint.buildFromOxlintConfigFile('.oxlintrc.json'),

  skipFormatting,
)
