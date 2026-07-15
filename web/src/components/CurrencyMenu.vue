<script setup lang="ts">
import { ref } from 'vue'
import { CircleDollarSign } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useCurrency, useSetCurrencyMutation } from '@/composables/useCurrency'
import { ApiError } from '@/lib/api'
import { CURRENCY_OPTIONS, isSupportedCurrency } from '@/lib/currency'
import { useAuthStore } from '@/stores/auth'

// Header shortcut for the same server-persisted preference as Settings. Signed-out
// visitors have no account preference to mutate, so the control appears only after the
// session resolves signed in.
const auth = useAuthStore()
const money = useCurrency()
const mutation = useSetCurrencyMutation()
const error = ref<string | null>(null)

async function onSelect(value: unknown) {
  if (!isSupportedCurrency(value) || value === money.currency.value) return
  error.value = null
  try {
    await mutation.mutateAsync({ currency: value })
  } catch (cause) {
    error.value =
      cause instanceof ApiError ? cause.message : 'Could not save the currency. Please try again.'
  }
}
</script>

<template>
  <DropdownMenu v-if="auth.isAuthenticated">
    <DropdownMenuTrigger as-child>
      <Button
        variant="ghost"
        size="sm"
        class="h-9 gap-1.5 px-2"
        :disabled="mutation.isPending.value"
      >
        <CircleDollarSign class="size-4" aria-hidden="true" />
        <span class="hidden text-xs font-semibold sm:inline">{{ money.currency.value }}</span>
        <span class="sr-only">Display currency: {{ money.currency.value }}</span>
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end" class="w-56">
      <DropdownMenuLabel>Display currency</DropdownMenuLabel>
      <DropdownMenuSeparator />
      <DropdownMenuRadioGroup :model-value="money.currency.value" @update:model-value="onSelect">
        <DropdownMenuRadioItem
          v-for="option in CURRENCY_OPTIONS"
          :key="option.code"
          :value="option.code"
        >
          <span class="w-8 font-mono text-xs font-semibold">{{ option.code }}</span>
          {{ option.label }}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
      <template v-if="error">
        <DropdownMenuSeparator />
        <p class="text-destructive px-2 py-1.5 text-xs" role="alert">{{ error }}</p>
      </template>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
