<script setup lang="ts">
import type { NavigationMenuRootEmits, NavigationMenuRootProps } from 'reka-ui'
import type { HTMLAttributes } from 'vue'
import { computed } from 'vue'
import { NavigationMenuRoot, useForwardPropsEmits } from 'reka-ui'
import { cn } from '@/lib/utils'
import NavigationMenuViewport from './NavigationMenuViewport.vue'

const props = withDefaults(
  defineProps<NavigationMenuRootProps & { class?: HTMLAttributes['class']; viewport?: boolean }>(),
  { viewport: true },
)
const emits = defineEmits<NavigationMenuRootEmits>()

// Strip the local-only props before forwarding to the reka root (this project doesn't
// depend on @vueuse, so no reactiveOmit — a computed omit does the same job).
const delegatedProps = computed(() => {
  const { class: _class, viewport: _viewport, ...rest } = props
  return rest
})
const forwarded = useForwardPropsEmits(delegatedProps, emits)
</script>

<template>
  <NavigationMenuRoot
    data-slot="navigation-menu"
    :data-viewport="viewport"
    v-bind="forwarded"
    :class="
      cn(
        'group/navigation-menu relative flex max-w-max flex-1 items-center justify-center',
        props.class,
      )
    "
  >
    <slot />
    <NavigationMenuViewport v-if="viewport" />
  </NavigationMenuRoot>
</template>
