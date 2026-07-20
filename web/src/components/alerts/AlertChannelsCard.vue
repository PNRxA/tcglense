<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import {
  Bell,
  CalendarClock,
  Check,
  LoaderCircle,
  Send,
  Sparkles,
  TriangleAlert,
} from '@lucide/vue'
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
import { ApiError, type AlertTestChannel, type AlertTestResult } from '@/lib/api'

// The notification-channels settings card on the Alerts page. Free, self-service channels —
// a Discord incoming-webhook URL and a Telegram bot token + chat id — plus an optional email
// toggle (only when the deployment enables it). Each of Discord and Telegram has its own
// "Test" button that verifies just that setup; "Test all" probes every configured channel.

const channelsQuery = useAlertChannelsQuery()
const save = useSetAlertChannelsMutation()
const test = useTestAlertChannelsMutation()

// Local form state, seeded from the server settings whenever they (re)load, so the form
// prefills and a partial edit (e.g. toggling a channel) resubmits the other fields unchanged.
const discordWebhookUrl = ref('')
const discordEnabled = ref(true)
const telegramBotToken = ref('')
const telegramChatId = ref('')
const telegramEnabled = ref(true)
const emailEnabled = ref(false)
// Release heads-ups (opt-in, off by default): a day-before notification for a Secret Lair drop
// and for a new set, delivered over the same channels above.
const sldReleaseEnabled = ref(false)
const setReleaseEnabled = ref(false)

// The last-loaded server settings, used to gate the per-channel Test buttons (they fire the
// *saved* credentials, not the unsaved form values).
const serverChannels = computed(() => channelsQuery.data.value)
const emailAvailable = computed(() => serverChannels.value?.email_available ?? false)

watch(
  () => channelsQuery.data.value,
  (data) => {
    if (!data) return
    discordWebhookUrl.value = data.discord_webhook_url ?? ''
    discordEnabled.value = data.discord_enabled
    telegramBotToken.value = data.telegram_bot_token ?? ''
    telegramChatId.value = data.telegram_chat_id ?? ''
    telegramEnabled.value = data.telegram_enabled
    emailEnabled.value = data.email_enabled
    sldReleaseEnabled.value = data.sld_release_enabled
    setReleaseEnabled.value = data.set_release_enabled
  },
  { immediate: true },
)

const saveError = ref<string | null>(null)
const saved = ref(false)

// "Test all" results box (one row per configured channel).
const testResults = ref<AlertTestResult[] | null>(null)
const testError = ref<string | null>(null)

// Per-channel "Test" outcome, shown inline beside that channel's own Test button.
interface ChannelTestState {
  kind: 'ok' | 'fail' | 'empty'
  detail?: string | null
}
const channelTest = reactive<Record<string, ChannelTestState | undefined>>({})
// Which test is currently in flight — a single channel, or 'all' — for the button spinners.
const pending = ref<AlertTestChannel | 'all' | null>(null)

// A per-channel Test button fires the *saved* credentials, so it's live only when that channel
// is saved (credentials present) AND enabled AND the form has no unsaved edits to it — otherwise
// the test would verify a stale/absent value, and the inline status explains why.
const discordConfigured = computed(() => !!serverChannels.value?.discord_webhook_url)
const telegramConfigured = computed(
  () => !!serverChannels.value?.telegram_bot_token && !!serverChannels.value?.telegram_chat_id,
)
const discordDirty = computed(() => {
  const s = serverChannels.value
  return (
    (discordWebhookUrl.value.trim() || null) !== (s?.discord_webhook_url ?? null) ||
    discordEnabled.value !== (s?.discord_enabled ?? true)
  )
})
const telegramDirty = computed(() => {
  const s = serverChannels.value
  return (
    (telegramBotToken.value.trim() || null) !== (s?.telegram_bot_token ?? null) ||
    (telegramChatId.value.trim() || null) !== (s?.telegram_chat_id ?? null) ||
    telegramEnabled.value !== (s?.telegram_enabled ?? true)
  )
})
const canTestDiscord = computed(
  () => discordConfigured.value && !!serverChannels.value?.discord_enabled && !discordDirty.value,
)
const canTestTelegram = computed(
  () =>
    telegramConfigured.value && !!serverChannels.value?.telegram_enabled && !telegramDirty.value,
)

// The single inline line shown beneath a channel. Priority: the unsaved-edit hint wins (so a
// stale "Test sent" never lingers next to a value the test didn't verify), then the last test
// result, then a configured/enabled hint that explains a disabled Test button.
type StatusTone = 'ok' | 'fail' | 'muted'
interface ChannelStatus {
  tone: StatusTone
  text: string
}
function channelStatus(args: {
  name: string
  loaded: boolean
  configured: boolean
  enabledSaved: boolean
  dirty: boolean
  result: ChannelTestState | undefined
  unconfiguredText: string
}): ChannelStatus | null {
  const { name, loaded, configured, enabledSaved, dirty, result, unconfiguredText } = args
  if (dirty) return { tone: 'muted', text: 'Save your changes to test them.' }
  if (result?.kind === 'ok') return { tone: 'ok', text: `Test sent — check ${name}.` }
  if (result?.kind === 'fail') return { tone: 'fail', text: result.detail || 'Test failed.' }
  if (result?.kind === 'empty' || (loaded && !configured))
    return { tone: 'muted', text: unconfiguredText }
  if (loaded && !enabledSaved) return { tone: 'muted', text: `Enable ${name} and save to test it.` }
  return null
}
const discordStatus = computed(() =>
  channelStatus({
    name: 'Discord',
    loaded: !!serverChannels.value,
    configured: discordConfigured.value,
    enabledSaved: !!serverChannels.value?.discord_enabled,
    dirty: discordDirty.value,
    result: channelTest.discord,
    unconfiguredText: 'Add a webhook URL and save it first.',
  }),
)
const telegramStatus = computed(() =>
  channelStatus({
    name: 'Telegram',
    loaded: !!serverChannels.value,
    configured: telegramConfigured.value,
    enabledSaved: !!serverChannels.value?.telegram_enabled,
    dirty: telegramDirty.value,
    result: channelTest.telegram,
    unconfiguredText: 'Add a bot token and chat id and save them first.',
  }),
)

function clearAllTestState() {
  testResults.value = null
  testError.value = null
  channelTest.discord = undefined
  channelTest.telegram = undefined
  channelTest.email = undefined
}

async function onSave() {
  saveError.value = null
  saved.value = false
  clearAllTestState()
  try {
    await save.mutateAsync({
      discord_webhook_url: discordWebhookUrl.value.trim() || null,
      discord_enabled: discordEnabled.value,
      telegram_bot_token: telegramBotToken.value.trim() || null,
      telegram_chat_id: telegramChatId.value.trim() || null,
      telegram_enabled: telegramEnabled.value,
      email_enabled: emailEnabled.value,
      sld_release_enabled: sldReleaseEnabled.value,
      set_release_enabled: setReleaseEnabled.value,
    })
    saved.value = true
  } catch (err) {
    saveError.value =
      err instanceof ApiError ? err.message : 'Could not save your channels. Please try again.'
  }
}

/**
 * Test one channel using its saved credentials; the result shows inline beside its button.
 * Clears the "Test all" box and this channel's own slot only — a sibling channel's inline
 * result is independent and left untouched.
 */
async function onTestChannel(channel: AlertTestChannel) {
  testResults.value = null
  testError.value = null
  channelTest[channel] = undefined
  pending.value = channel
  try {
    const response = await test.mutateAsync(channel)
    const result = response.results[0]
    channelTest[channel] = result
      ? { kind: result.ok ? 'ok' : 'fail', detail: result.detail }
      : { kind: 'empty' }
  } catch (err) {
    testError.value =
      err instanceof ApiError ? err.message : 'Could not send a test. Please try again.'
  } finally {
    pending.value = null
  }
}

/** Test every configured channel at once; the outcomes show in the box below. */
async function onTestAll() {
  clearAllTestState()
  pending.value = 'all'
  try {
    const response = await test.mutateAsync(undefined)
    testResults.value = response.results
  } catch (err) {
    testError.value =
      err instanceof ApiError ? err.message : 'Could not send a test. Please try again.'
  } finally {
    pending.value = null
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
        Where your alerts and release heads-ups are delivered. Discord and Telegram are free and set
        up in a couple of minutes; nothing is sent until an alert fires.
      </CardDescription>
    </CardHeader>
    <CardContent>
      <form class="grid gap-5" @submit.prevent="onSave">
        <!-- Discord -->
        <div class="space-y-1.5">
          <div class="flex items-center justify-between gap-2">
            <Label for="discord-webhook">Discord webhook URL</Label>
            <div class="flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                :disabled="!canTestDiscord || pending !== null"
                @click="onTestChannel('discord')"
              >
                <LoaderCircle v-if="pending === 'discord'" class="animate-spin" />
                <Send v-else class="size-4" />
                Test
              </Button>
              <Switch
                :checked="discordEnabled"
                aria-label="Discord alerts"
                @update:checked="discordEnabled = $event"
              />
            </div>
          </div>
          <Input
            id="discord-webhook"
            v-model="discordWebhookUrl"
            :disabled="!discordEnabled"
            placeholder="https://discord.com/api/webhooks/…"
            autocomplete="off"
            spellcheck="false"
          />
          <p class="text-muted-foreground text-xs">
            In your Discord server: Settings → Integrations → Webhooks → New Webhook → Copy URL.
          </p>
          <p
            v-if="discordStatus"
            :class="[
              'flex items-start gap-1.5 text-xs',
              discordStatus.tone === 'ok'
                ? 'text-emerald-600 dark:text-emerald-400'
                : discordStatus.tone === 'fail'
                  ? 'text-destructive'
                  : 'text-muted-foreground',
            ]"
            :role="discordStatus.tone === 'fail' ? 'alert' : undefined"
          >
            <Check v-if="discordStatus.tone === 'ok'" class="mt-0.5 size-3.5 shrink-0" />
            <TriangleAlert
              v-else-if="discordStatus.tone === 'fail'"
              class="mt-0.5 size-3.5 shrink-0"
            />
            <span>{{ discordStatus.text }}</span>
          </p>
        </div>

        <!-- Telegram -->
        <div class="grid gap-3 sm:grid-cols-2">
          <div class="flex items-center justify-between gap-2 sm:col-span-2">
            <span class="text-sm font-medium">Telegram</span>
            <div class="flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                :disabled="!canTestTelegram || pending !== null"
                @click="onTestChannel('telegram')"
              >
                <LoaderCircle v-if="pending === 'telegram'" class="animate-spin" />
                <Send v-else class="size-4" />
                Test
              </Button>
              <Switch
                :checked="telegramEnabled"
                aria-label="Telegram alerts"
                @update:checked="telegramEnabled = $event"
              />
            </div>
          </div>
          <div class="space-y-1.5">
            <Label for="telegram-token">Telegram bot token</Label>
            <Input
              id="telegram-token"
              v-model="telegramBotToken"
              :disabled="!telegramEnabled"
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
              :disabled="!telegramEnabled"
              placeholder="e.g. 987654321"
              autocomplete="off"
              spellcheck="false"
            />
          </div>
          <p class="text-muted-foreground text-xs sm:col-span-2">
            Create a bot with <span class="font-mono">@BotFather</span> for the token, then message
            your bot and read your chat id from <span class="font-mono">@userinfobot</span>.
          </p>
          <p
            v-if="telegramStatus"
            :class="[
              'flex items-start gap-1.5 text-xs sm:col-span-2',
              telegramStatus.tone === 'ok'
                ? 'text-emerald-600 dark:text-emerald-400'
                : telegramStatus.tone === 'fail'
                  ? 'text-destructive'
                  : 'text-muted-foreground',
            ]"
            :role="telegramStatus.tone === 'fail' ? 'alert' : undefined"
          >
            <Check v-if="telegramStatus.tone === 'ok'" class="mt-0.5 size-3.5 shrink-0" />
            <TriangleAlert
              v-else-if="telegramStatus.tone === 'fail'"
              class="mt-0.5 size-3.5 shrink-0"
            />
            <span>{{ telegramStatus.text }}</span>
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

        <!-- Release heads-ups (opt-in): delivered over the channels above, a day before. -->
        <div class="space-y-2 border-t pt-4">
          <div class="space-y-0.5">
            <p class="text-sm font-medium">Release heads-ups</p>
            <p class="text-muted-foreground text-xs">
              A notification the day before, over the channels above. Nothing else changes what you
              already track.
            </p>
          </div>
          <div class="flex items-center justify-between gap-4 rounded-lg border p-3">
            <div class="flex items-center gap-2.5">
              <Sparkles class="text-muted-foreground size-4 shrink-0" />
              <div class="space-y-0.5">
                <Label for="sld-release" class="font-medium">Secret Lair drops</Label>
                <p class="text-muted-foreground text-xs">
                  When a new Secret Lair drop is about to release.
                </p>
              </div>
            </div>
            <Switch
              id="sld-release"
              :checked="sldReleaseEnabled"
              aria-label="Secret Lair drop releases"
              @update:checked="sldReleaseEnabled = $event"
            />
          </div>
          <div class="flex items-center justify-between gap-4 rounded-lg border p-3">
            <div class="flex items-center gap-2.5">
              <CalendarClock class="text-muted-foreground size-4 shrink-0" />
              <div class="space-y-0.5">
                <Label for="set-release" class="font-medium">New set releases</Label>
                <p class="text-muted-foreground text-xs">
                  One heads-up per new set (e.g. an upcoming expansion), not per product.
                </p>
              </div>
            </div>
            <Switch
              id="set-release"
              :checked="setReleaseEnabled"
              aria-label="New set releases"
              @update:checked="setReleaseEnabled = $event"
            />
          </div>
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

        <!-- "Test all" results -->
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
          <Button type="button" variant="outline" :disabled="pending !== null" @click="onTestAll">
            <LoaderCircle v-if="pending === 'all'" class="animate-spin" />
            <Send v-else class="size-4" />
            Test all
          </Button>
        </div>
      </form>
    </CardContent>
  </Card>
</template>
