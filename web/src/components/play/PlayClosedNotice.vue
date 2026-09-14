<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { Button } from '@/components/ui/button'

// "This room is no longer available", wherever the player happens to be standing.
//
// A `4xxx` close is final: the socket will not come back, so whatever is still drawn behind
// this is a photograph of a table nobody is sitting at any more. That makes *reachability*
// the whole job — the message used to live only in the lobby branch of the room page, which
// meant the one moment it mattered most (the host closing the room while everyone was
// playing) rendered it underneath a full-screen table nobody could dismiss.
//
// So it renders two ways from one definition: in the page while the lobby is up, and as an
// overlay above the table when the table owns the viewport. `z-[60]` clears the table's own
// `z-50` layer, and the link out is a real link, because the only useful next step is to
// leave.
defineProps<{ reason: string; backTo: string; overlay?: boolean }>()
</script>

<template>
  <div :class="overlay ? 'bg-background/90 fixed inset-0 z-[60] grid place-items-center p-4' : ''">
    <div
      class="bg-card w-full rounded-xl border p-8 text-center"
      :class="overlay ? 'max-w-md shadow-xl' : ''"
    >
      <p class="font-medium">This room is no longer available</p>
      <p class="text-muted-foreground mx-auto mt-2 max-w-sm text-sm">{{ reason }}</p>
      <Button class="mt-4" variant="outline" as-child>
        <RouterLink :to="backTo">Back to Play online</RouterLink>
      </Button>
    </div>
  </div>
</template>
