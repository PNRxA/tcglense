<script setup lang="ts">
import { computed, onScopeDispose, ref } from 'vue'
import { Check, Copy, Share2 } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { playRoomPath } from '@/lib/tools'

// The invite: the room's whole social mechanic in one box.
//
// Both halves are shown because they're used differently — the URL is what you paste into a
// group chat, the code is what you read out loud over voice while someone types it into the
// hub's join box. The code is spelled in a wide-tracked monospace for exactly that: it's read
// character by character, and a room code misheard is a room you can't get into.
//
// `navigator.share` is offered when the browser has it (phones, mostly) because that's the one
// place a native share sheet genuinely beats a copy button. It's additive, never a replacement:
// the copy button is always there.
const props = defineProps<{ game: string; code: string }>()

const url = computed(() => {
  const path = playRoomPath(props.game, props.code)
  return typeof window === 'undefined' ? path : `${window.location.origin}${path}`
})

const copied = ref(false)
let copiedTimer: ReturnType<typeof setTimeout> | undefined
onScopeDispose(() => clearTimeout(copiedTimer))

const canShare = computed(() => typeof navigator !== 'undefined' && Boolean(navigator.share))

async function copy() {
  try {
    await navigator.clipboard.writeText(url.value)
    copied.value = true
    clearTimeout(copiedTimer)
    copiedTimer = setTimeout(() => {
      copied.value = false
    }, 2000)
  } catch {
    // Clipboard blocked (insecure origin, denied permission): the link is on screen and
    // selectable, so there is nothing useful to say here.
  }
}

async function share() {
  try {
    await navigator.share({ title: 'Play a game on TCGLense', url: url.value })
  } catch {
    // Dismissing the share sheet rejects; that's a choice, not an error.
  }
}
</script>

<template>
  <div class="bg-card rounded-xl border p-4">
    <p class="text-sm font-medium">Invite your table</p>
    <p class="text-muted-foreground mt-1 text-sm">
      Anyone with this link can take a seat — no account needed.
    </p>

    <div class="mt-3 flex flex-wrap items-center gap-2">
      <code
        class="bg-muted min-w-0 flex-1 truncate rounded-md px-3 py-2 font-mono text-sm"
        data-testid="play-invite-url"
        >{{ url }}</code
      >
      <Button variant="outline" size="sm" :aria-label="'Copy the invite link'" @click="copy">
        <Check v-if="copied" class="size-4 text-success" aria-hidden="true" />
        <Copy v-else class="size-4" aria-hidden="true" />
        {{ copied ? 'Copied' : 'Copy' }}
      </Button>
      <Button
        v-if="canShare"
        variant="outline"
        size="sm"
        aria-label="Share the invite"
        @click="share"
      >
        <Share2 class="size-4" aria-hidden="true" /> Share
      </Button>
    </div>

    <p class="text-muted-foreground mt-3 text-sm">
      Or give them the code:
      <span class="text-foreground ml-1 font-mono text-base font-semibold tracking-[0.3em]">{{
        code
      }}</span>
    </p>
  </div>
</template>
