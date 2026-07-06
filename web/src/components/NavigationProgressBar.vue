<script setup lang="ts">
import { onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'

// A thin indeterminate progress bar pinned to the top of the viewport while a route
// navigation is in flight. It's armed on a delay so FAST navigations never flash it —
// cached chunks and query-only navs (pagination via router.replace, the ?card= dialog)
// resolve well under the delay, so only genuinely slow chunk/data waits surface a bar.
const SHOW_DELAY_MS = 120

const router = useRouter()
const visible = ref(false)
let timer: ReturnType<typeof setTimeout> | null = null

function clearTimer() {
  if (timer !== null) {
    clearTimeout(timer)
    timer = null
  }
}

function hide() {
  clearTimer()
  visible.value = false
}

// beforeEach (re)arms the delayed show on every navigation. afterEach fires only for the
// FINAL target of a redirect chain, so it's a safe single clear; onError covers an
// aborted/failed navigation that never reaches afterEach. All three unregister on unmount.
const stopBeforeEach = router.beforeEach(() => {
  clearTimer()
  timer = setTimeout(() => {
    visible.value = true
  }, SHOW_DELAY_MS)
})
const stopAfterEach = router.afterEach(() => hide())
const stopOnError = router.onError(() => hide())

onUnmounted(() => {
  stopBeforeEach()
  stopAfterEach()
  stopOnError()
  clearTimer()
})
</script>

<template>
  <div
    v-if="visible"
    class="fixed top-0 right-0 left-0 z-50 h-[2.5px] overflow-hidden"
    role="progressbar"
    aria-label="Loading page"
  >
    <!-- Indeterminate trickle: the bar eases from a sliver toward ~92% and holds, so a
         long wait keeps creeping without ever claiming completion (afterEach unmounts it
         on arrival). Reduced-motion users get a static full-width bar instead. -->
    <div class="progress-bar bg-primary h-full" />
  </div>
</template>

<style scoped>
.progress-bar {
  animation: nav-progress 12s ease-out forwards;
}

@keyframes nav-progress {
  0% {
    width: 8%;
  }
  40% {
    width: 55%;
  }
  70% {
    width: 78%;
  }
  100% {
    width: 92%;
  }
}

@media (prefers-reduced-motion: reduce) {
  .progress-bar {
    width: 100%;
    animation: none;
  }
}
</style>
