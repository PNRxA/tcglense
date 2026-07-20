<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Bell, Check, LoaderCircle, Send, TriangleAlert } from '@lucide/vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  useAlertChannelsQuery,
  useSetAlertChannelsMutation,
  useTestAlertChannelsMutation,
} from '@/composables/useAlerts'
import { ApiError, type AlertTestResult } from '@/lib/api'

// The notification-channels settings card on the Alerts page. Free, self-service channels —
// a Discord incoming-webhook URL and a Telegram bot token + chat id — plus an optional email
// toggle (only when the deployment enables it). "Send test" verifies a setup end to end.

const channelsQuery = useAlertChannelsQuery()
const save = useSetAlertChannelsMutation()
const test = useTestAlertChannelsMutation()

// Local form state, seeded from the server settings whenever they (re)load, so the form
// prefills and a partial edit (e.g. toggling email) resubmits the other fields unchanged.
const discordWebhookUrl = ref('')
const telegramBotToken = ref('')
const telegramChatId = ref('')
const emailEnabled = ref(false)

const emailAvailable = computed(() => channelsQuery.data.value?.email_available ?? false)

watch(
  () => channelsQuery.data.value,
  (data) => {
    if (!data) return
    discordWebhookUrl.value = data.discord_webhook_url ?? ''
    telegramBotToken.value = data.telegram_bot_token ?? ''
    telegramChatId.value = data.telegram_chat_id ?? ''
    emailEnabled.value = data.email_enabled
  },
  { immediate: true },
)

const saveError = ref<string | null>(null)
const saved = ref(false)
const testResults = ref<AlertTestResult[] | null>(null)
const testError = ref<string | null>(null)

async function onSave() {
  saveError.value = null
  saved.value = false
  testResults.value = null
  try {
    await save.mutateAsync({
      discord_webhook_url: discordWebhookUrl.value.trim() || null,
      telegram_bot_token: telegramBotToken.value.trim() || null,
      telegram_chat_id: telegramChatId.value.trim() || null,
      email_enabled: emailEnabled.value,
    })
    saved.value = true
  } catch (err) {
    saveError.value =
      err instanceof ApiError ? err.message : 'Could not save your channels. Please try again.'
  }
}

async function onTest() {
  testError.value = null
  testResults.value = null
  try {
    const response = await test.mutateAsync()
    testResults.value = response.results
  } catch (err) {
    testError.value =
      err instanceof ApiError ? err.message : 'Could not send a test. Please try again.'
  }
}
</script>

<template>
  <Card>
    <CardHeader>
      <CardTitle class="flex items-center gap-2 text-lg">
        <Bell class="size-5" /> Notification channels
      </CardTitle>
      <CardDescription>
        Where your triggered price alerts are delivered. Discord and Telegram are free and set up in
        a couple of minutes; nothing is sent until an alert fires.
      </CardDescription>
    </CardHeader>
    <CardContent>
      <form class="grid gap-5" @submit.prevent="onSave">
        <!-- Discord -->
        <div class="space-y-1.5">
          <Label for="discord-webhook">Discord webhook URL</Label>
          <Input
            id="discord-webhook"
            v-model="discordWebhookUrl"
            placeholder="https://discord.com/api/webhooks/…"
            autocomplete="off"
            spellcheck="false"
          />
          <p class="text-muted-foreground text-xs">
            In your Discord server: Settings → Integrations → Webhooks → New Webhook → Copy URL.
          </p>
        </div>

        <!-- Telegram -->
        <div class="grid gap-3 sm:grid-cols-2">
          <div class="space-y-1.5">
            <Label for="telegram-token">Telegram bot token</Label>
            <Input
              id="telegram-token"
              v-model="telegramBotToken"
              placeholder="123456:ABC-DEF…"
              autocomplete="off"
              spellcheck="false"
            />
          </div>
          <div class="space-y-1.5">
            <Label for="telegram-chat">Telegram chat id</Label>
            <Input
              id="telegram-chat"
              v-model="telegramChatId"
              placeholder="e.g. 987654321"
              autocomplete="off"
              spellcheck="false"
            />
          </div>
          <p class="text-muted-foreground text-xs sm:col-span-2">
            Create a bot with <span class="font-mono">@BotFather</span> for the token, then message
            your bot and read your chat id from <span class="font-mono">@userinfobot</span>.
          </p>
        </div>

        <!-- Email (only when the deployment offers it) -->
        <div
          v-if="emailAvailable"
          class="flex items-center justify-between gap-4 rounded-lg border p-3"
        >
          <div class="space-y-0.5">
            <Label for="email-alerts" class="font-medium">Email alerts</Label>
            <p class="text-muted-foreground text-xs">Sent to your account email address.</p>
          </div>
          <Switch
            id="email-alerts"
            :checked="emailEnabled"
            @update:checked="emailEnabled = $event"
          />
        </div>

        <!-- Feedback -->
        <p v-if="saveError" class="text-destructive flex items-start gap-1.5 text-sm" role="alert">
          <TriangleAlert class="mt-0.5 size-4 shrink-0" />
          <span>{{ saveError }}</span>
        </p>
        <p
          v-else-if="saved"
          class="flex items-center gap-1.5 text-sm text-emerald-600 dark:text-emerald-400"
        >
          <Check class="size-4" /> Channels saved.
        </p>

        <!-- Test results -->
        <div v-if="testResults" class="rounded-lg border p-3">
          <p v-if="testResults.length === 0" class="text-muted-foreground text-sm">
            No channels configured yet — add one above and save first.
          </p>
          <ul v-else class="space-y-1 text-sm">
            <li v-for="result in testResults" :key="result.channel" class="flex items-center gap-2">
              <Check v-if="result.ok" class="size-4 text-emerald-600 dark:text-emerald-400" />
              <TriangleAlert v-else class="text-destructive size-4" />
              <span class="capitalize">{{ result.channel }}</span>
              <span v-if="!result.ok && result.detail" class="text-muted-foreground text-xs">
                — {{ result.detail }}
              </span>
            </li>
          </ul>
        </div>
        <p v-if="testError" class="text-destructive text-sm" role="alert">{{ testError }}</p>

        <div class="flex flex-wrap gap-2">
          <Button type="submit" :disabled="save.isPending.value">
            <LoaderCircle v-if="save.isPending.value" class="animate-spin" />
            Save channels
          </Button>
          <Button type="button" variant="outline" :disabled="test.isPending.value" @click="onTest">
            <LoaderCircle v-if="test.isPending.value" class="animate-spin" />
            <Send v-else class="size-4" />
            Send test
          </Button>
        </div>
      </form>
    </CardContent>
  </Card>
</template>
