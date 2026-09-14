<script setup lang="ts">
import { ContextMenuRoot } from 'reka-ui'

// Hand-written (the shadcn-vue registry was unreachable when this was added) over reka-ui's
// `ContextMenu*` primitives, mirroring the sibling `ui/dropdown-menu` exactly: same wrapper
// shape, same `data-slot` attributes, same item classes. The difference is the trigger —
// a context menu opens on right-click, on the Menu/Shift+F10 key, and on a long press
// (reka's `pressOpenDelay`), which is what a table of cards wants.
</script>

<template>
  <ContextMenuRoot data-slot="context-menu">
    <slot />
  </ContextMenuRoot>
</template>
