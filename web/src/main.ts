// Self-hosted Inter (variable weight), the brand sans font applied via --font-sans in
// main.css. Bundled into the build's render-blocking stylesheet, so there's no
// third-party request on the critical path (the old Google Fonts @import was a
// render-blocking hop that, moreover, was never actually applied).
import '@fontsource-variable/inter/index.css'
import './assets/main.css'
// MTG mana/cost symbol icon font (used by ManaSymbols.vue to render `{W}`, `{T}`, …).
// The woff2 override must load AFTER the package CSS to win the "Mana" @font-face.
import 'mana-font/css/mana.css'
import './assets/mana-font.css'

import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { VueQueryPlugin } from '@tanstack/vue-query'

import App from './App.vue'
import router from './router'
import { createQueryClient } from './lib/queryClient'

const app = createApp(App)

app.use(createPinia())
app.use(router)
// vue-query owns server state (prices, collection, set-completion); Pinia keeps
// owning auth/session and other client state.
app.use(VueQueryPlugin, { queryClient: createQueryClient() })

app.mount('#app')
