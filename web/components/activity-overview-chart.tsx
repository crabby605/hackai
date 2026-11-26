"use client"

import { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart"
import { Bar, CartesianGrid, ComposedChart, Line, XAxis, YAxis } from "recharts"

const chartConfig = {
  human: { label: "Human time", color: "#22c55e" },
  agent: { label: "Agent time", color: "#8b5cf6" },
  pesdo: { label: "PESDO", color: "#f59e0b" },
} satisfies ChartConfig

export interface ActivityOverviewPoint {
  label: string
  human: number
  agent: number
  pesdo: number | null
  recordedAt: string
}

export function ActivityOverviewChart({ data }: { data: ActivityOverviewPoint[] }) {
  return <ChartContainer config={chartConfig} className="h-56 w-full">
    <ComposedChart accessibilityLayer data={data} margin={{ left: 2, right: 4, top: 16 }}>
      <CartesianGrid vertical={false} />
      <XAxis dataKey="label" tickLine={false} axisLine={false} tickMargin={8} />
      <YAxis yAxisId="time" tickLine={false} axisLine={false} tickMargin={8} width={32} tickFormatter={(value) => `${value}m`} />
      <YAxis yAxisId="score" orientation="right" domain={[0, 1000]} ticks={[0, 500, 1000]} tickLine={false} axisLine={false} tickMargin={8} width={34} />
      <ChartTooltip cursor={false} content={<ChartTooltipContent labelFormatter={(_, payload) => payload[0]?.payload.recordedAt ?? ""} />} />
      <Bar yAxisId="time" dataKey="human" stackId="time" barSize={14} fill="var(--color-human)" radius={[3, 3, 0, 0]} />
      <Bar yAxisId="time" dataKey="agent" stackId="time" barSize={14} fill="var(--color-agent)" radius={[3, 3, 0, 0]} />
      <Line yAxisId="score" dataKey="pesdo" type="monotone" stroke="var(--color-pesdo)" strokeWidth={2.5} dot={{ r: 4, fill: "var(--color-pesdo)", strokeWidth: 0 }} activeDot={{ r: 6 }} connectNulls />
      <ChartLegend content={<ChartLegendContent />} />
    </ComposedChart>
  </ChartContainer>
}
