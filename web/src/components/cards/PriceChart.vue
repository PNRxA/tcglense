<script setup lang="ts">
import { computed, toRef } from 'vue'
import { useQuery } from '@tanstack/vue-query'
import { VisAxis, VisLine, VisScatter, VisXYContainer } from '@unovis/vue'
import {
  type ChartConfig,
  ChartContainer,
  ChartCrosshair,
  ChartTooltip,
  ChartTooltipContent,
  componentToString,
} from '@/components/ui/chart'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { getPriceHistory } from '@/lib/api'

const props = defineProps<{ game: string; id: string }>()
const game = toRef(props, 'game')
const id = toRef(props, 'id')

// Public price-history endpoint, so a plain useQuery (no auth wrapper). Refs go
// straight into the queryKey so a card-to-card navigation refetches.
const query = useQuery({
  queryKey: ['card-prices', game, id],
  queryFn: () => getPriceHistory(game.value, id.value),
  staleTime: 5 * 60 * 1000,
})

// One plotted day. Dates become epoch ms for a continuous x-scale; price strings
// become numbers, with null kept as null so the line *gaps* over missing days
// rather than dropping to zero.
interface PricePlot {
  date: number
  usd: number | null
  usdFoil: number | null
}

function toNumber(value: string | null): number | null {
  if (value == null) return null
  const n = Number.parseFloat(value)
  return Number.isFinite(n) ? n : null
}

const points = computed<PricePlot[]>(() =>
  (query.data.value?.data ?? []).map((p) => ({
    date: new Date(p.date).getTime(),
    usd: toNumber(p.usd),
    usdFoil: toNumber(p.usd_foil),
  })),
)

const isEmpty = computed(
  () => !query.isPending.value && !query.isError.value && points.value.length === 0,
)

// Series legend/tooltip metadata. Colours are the theme's chart tokens, which the
// CSS variables resolve differently in light vs dark, so the chart follows the theme.
const chartConfig = {
  usd: { label: 'USD', color: 'var(--chart-1)' },
  usdFoil: { label: 'USD foil', color: 'var(--chart-2)' },
} satisfies ChartConfig

const x = (d: PricePlot) => d.date
const usdY = (d: PricePlot) => d.usd
const foilY = (d: PricePlot) => d.usdFoil

const dateFmt = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  year: 'numeric',
})
const formatDate = (tick: number | Date) =>
  dateFmt.format(typeof tick === 'number' ? new Date(tick) : tick)
const formatPrice = (tick: number | Date) =>
  `$${Number(tick).toLocaleString(undefined, { maximumFractionDigits: 2 })}`

// Rich tooltip rendered from the shadcn primitive (built once during setup).
const tooltipTemplate = componentToString(chartConfig, ChartTooltipContent, {
  labelFormatter: formatDate,
  indicator: 'line',
})
</script>

<template>
  <Card class="mt-6">
    <CardHeader>
      <CardTitle class="text-sm font-semibold">Price history</CardTitle>
    </CardHeader>
    <CardContent>
      <div
        v-if="query.isPending.value"
        class="bg-muted/40 h-64 w-full animate-pulse rounded-xl"
        aria-hidden="true"
      />
      <p v-else-if="query.isError.value" class="text-muted-foreground py-12 text-sm">
        Couldn't load price history.
      </p>
      <p v-else-if="isEmpty" class="text-muted-foreground py-12 text-sm">No price history yet.</p>

      <ChartContainer v-else :config="chartConfig" class="aspect-auto h-64 w-full" :cursor="true">
        <VisXYContainer :data="points" :margin="{ left: 8, right: 8 }">
          <VisLine :x="x" :y="usdY" color="var(--chart-1)" :line-width="2" />
          <VisLine :x="x" :y="foilY" color="var(--chart-2)" :line-width="2" />
          <VisScatter :x="x" :y="usdY" color="var(--chart-1)" :size="36" />
          <VisScatter :x="x" :y="foilY" color="var(--chart-2)" :size="36" />
          <VisAxis
            type="x"
            :x="x"
            :tick-format="formatDate"
            :num-ticks="5"
            :grid-line="false"
            :tick-line="false"
            :domain-line="false"
          />
          <VisAxis
            type="y"
            :tick-format="formatPrice"
            :grid-line="true"
            :tick-line="false"
            :domain-line="false"
          />
          <ChartCrosshair color="var(--chart-1)" :template="tooltipTemplate" />
          <ChartTooltip />
        </VisXYContainer>
      </ChartContainer>
    </CardContent>
  </Card>
</template>
