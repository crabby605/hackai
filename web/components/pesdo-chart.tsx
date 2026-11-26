"use client"

import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart"
import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from "recharts"

const chartConfig = {
  score: { label: "PESDO score", color: "#8b5cf6" },
} satisfies ChartConfig

export interface PesdoChartPoint {
  episode: string
  score: number
  recordedAt: string
}

export function PesdoChart({ data }: { data: PesdoChartPoint[] }) {
  return <ChartContainer config={chartConfig} className="h-52 w-full">
    <AreaChart accessibilityLayer data={data} margin={{ left: 4, right: 12, top: 12 }}>
      <defs><linearGradient id="pesdo-score-fill" x1="0" x2="0" y1="0" y2="1"><stop offset="5%" stopColor="var(--color-score)" stopOpacity={0.36} /><stop offset="95%" stopColor="var(--color-score)" stopOpacity={0.02} /></linearGradient></defs>
      <CartesianGrid vertical={false} />
      <XAxis dataKey="episode" tickLine={false} axisLine={false} tickMargin={8} />
      <YAxis domain={[0, 1000]} ticks={[0, 500, 1000]} tickLine={false} axisLine={false} tickMargin={8} width={32} />
      <ChartTooltip cursor={false} content={<ChartTooltipContent labelFormatter={(_, payload) => payload[0]?.payload.recordedAt ?? ""} />} />
      <Area dataKey="score" type="monotone" stroke="var(--color-score)" strokeWidth={2.5} fill="url(#pesdo-score-fill)" dot={{ r: 4, fill: "var(--color-score)", strokeWidth: 0 }} activeDot={{ r: 6 }} />
    </AreaChart>
  </ChartContainer>
}
