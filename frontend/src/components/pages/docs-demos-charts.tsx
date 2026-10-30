"use client";

import { Area, AreaChart, Bar, BarChart, CartesianGrid, Line, LineChart, Pie, PieChart, XAxis, YAxis } from "recharts";
import {
  type ChartConfig,
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/shadcn/chart";
import type { DemoEntry } from "./docs-demo-types";

// The figures below are sample data for the examples, not measurements.

const barData = [
  { day: "Mon", completed: 42, failed: 3 },
  { day: "Tue", completed: 58, failed: 5 },
  { day: "Wed", completed: 37, failed: 2 },
  { day: "Thu", completed: 64, failed: 4 },
  { day: "Fri", completed: 71, failed: 6 },
  { day: "Sat", completed: 29, failed: 1 },
  { day: "Sun", completed: 33, failed: 2 },
];

const barConfig = {
  completed: { label: "Completed", color: "var(--chart-1)" },
  failed: { label: "Failed", color: "var(--chart-2)" },
} satisfies ChartConfig;

function BarDemo() {
  return (
    <ChartContainer config={barConfig} className="h-[260px] w-full">
      <BarChart accessibilityLayer data={barData}>
        <CartesianGrid vertical={false} />
        <XAxis dataKey="day" tickLine={false} axisLine={false} tickMargin={8} />
        <YAxis tickLine={false} axisLine={false} width={32} />
        <ChartTooltip content={<ChartTooltipContent />} />
        <ChartLegend content={<ChartLegendContent />} />
        <Bar dataKey="completed" fill="var(--color-completed)" radius={4} />
        <Bar dataKey="failed" fill="var(--color-failed)" radius={4} />
      </BarChart>
    </ChartContainer>
  );
}

const lineData = [
  { week: "W1", units: 24 },
  { week: "W2", units: 27 },
  { week: "W3", units: 22 },
  { week: "W4", units: 31 },
  { week: "W5", units: 29 },
  { week: "W6", units: 35 },
];

const lineConfig = {
  units: { label: "Usage units per replica", color: "var(--chart-1)" },
} satisfies ChartConfig;

function LineDemo() {
  return (
    <ChartContainer config={lineConfig} className="h-[260px] w-full">
      <LineChart accessibilityLayer data={lineData} margin={{ left: 12, right: 12 }}>
        <CartesianGrid vertical={false} />
        <XAxis dataKey="week" tickLine={false} axisLine={false} tickMargin={8} />
        <YAxis tickLine={false} axisLine={false} width={32} />
        <ChartTooltip cursor={false} content={<ChartTooltipContent />} />
        <Line dataKey="units" type="monotone" stroke="var(--color-units)" strokeWidth={2} dot={false} />
      </LineChart>
    </ChartContainer>
  );
}

const areaData = [
  { month: "Jan", usage: 186, fee: 19 },
  { month: "Feb", usage: 305, fee: 31 },
  { month: "Mar", usage: 237, fee: 24 },
  { month: "Apr", usage: 273, fee: 27 },
  { month: "May", usage: 209, fee: 21 },
  { month: "Jun", usage: 314, fee: 31 },
];

const areaConfig = {
  usage: { label: "Usage", color: "var(--chart-1)" },
  fee: { label: "Platform fee", color: "var(--chart-2)" },
} satisfies ChartConfig;

function AreaDemo() {
  return (
    <ChartContainer config={areaConfig} className="h-[260px] w-full">
      <AreaChart accessibilityLayer data={areaData} margin={{ left: 12, right: 12 }}>
        <CartesianGrid vertical={false} />
        <XAxis dataKey="month" tickLine={false} axisLine={false} tickMargin={8} />
        <ChartTooltip cursor={false} content={<ChartTooltipContent indicator="dot" />} />
        <Area dataKey="fee" type="natural" stackId="a" fill="var(--color-fee)" fillOpacity={0.4} stroke="var(--color-fee)" />
        <Area dataKey="usage" type="natural" stackId="a" fill="var(--color-usage)" fillOpacity={0.4} stroke="var(--color-usage)" />
      </AreaChart>
    </ChartContainer>
  );
}

const pieData = [
  { outcome: "completed", count: 62, fill: "var(--color-completed)" },
  { outcome: "stopped", count: 12, fill: "var(--color-stopped)" },
  { outcome: "failed", count: 6, fill: "var(--color-failed)" },
];

const pieConfig = {
  count: { label: "Tasks" },
  completed: { label: "Completed", color: "var(--chart-1)" },
  stopped: { label: "Stopped", color: "var(--chart-3)" },
  failed: { label: "Failed", color: "var(--chart-5)" },
} satisfies ChartConfig;

function PieDemo() {
  return (
    <ChartContainer config={pieConfig} className="mx-auto aspect-square h-[260px]">
      <PieChart>
        <ChartTooltip content={<ChartTooltipContent hideLabel nameKey="outcome" />} />
        <Pie data={pieData} dataKey="count" nameKey="outcome" innerRadius={62} outerRadius={96} strokeWidth={4} />
      </PieChart>
    </ChartContainer>
  );
}

export const CHART_DEMOS: Record<string, DemoEntry> = {
  "chart-bar": {
    title: { en: "Bar chart", zh: "長條圖" },
    code: `const barData = [
  { day: "Mon", completed: 42, failed: 3 },
  { day: "Tue", completed: 58, failed: 5 },
  // ...
]

const chartConfig = {
  completed: { label: "Completed", color: "var(--chart-1)" },
  failed: { label: "Failed", color: "var(--chart-2)" },
} satisfies ChartConfig

<ChartContainer config={chartConfig} className="h-[260px] w-full">
  <BarChart accessibilityLayer data={barData}>
    <CartesianGrid vertical={false} />
    <XAxis dataKey="day" tickLine={false} axisLine={false} tickMargin={8} />
    <ChartTooltip content={<ChartTooltipContent />} />
    <ChartLegend content={<ChartLegendContent />} />
    <Bar dataKey="completed" fill="var(--color-completed)" radius={4} />
    <Bar dataKey="failed" fill="var(--color-failed)" radius={4} />
  </BarChart>
</ChartContainer>`,
    Preview: BarDemo,
  },
  "chart-line": {
    title: { en: "Line chart", zh: "折線圖" },
    code: `const chartConfig = {
  units: { label: "Usage units per replica", color: "var(--chart-1)" },
} satisfies ChartConfig

<ChartContainer config={chartConfig} className="h-[260px] w-full">
  <LineChart accessibilityLayer data={lineData}>
    <CartesianGrid vertical={false} />
    <XAxis dataKey="week" tickLine={false} axisLine={false} tickMargin={8} />
    <ChartTooltip cursor={false} content={<ChartTooltipContent />} />
    <Line dataKey="units" type="monotone" stroke="var(--color-units)" strokeWidth={2} dot={false} />
  </LineChart>
</ChartContainer>`,
    Preview: LineDemo,
  },
  "chart-area": {
    title: { en: "Stacked area chart", zh: "堆疊面積圖" },
    code: `const chartConfig = {
  usage: { label: "Usage", color: "var(--chart-1)" },
  fee: { label: "Platform fee", color: "var(--chart-2)" },
} satisfies ChartConfig

<ChartContainer config={chartConfig} className="h-[260px] w-full">
  <AreaChart accessibilityLayer data={areaData}>
    <CartesianGrid vertical={false} />
    <XAxis dataKey="month" tickLine={false} axisLine={false} tickMargin={8} />
    <ChartTooltip cursor={false} content={<ChartTooltipContent indicator="dot" />} />
    <Area dataKey="fee" type="natural" stackId="a" fill="var(--color-fee)" fillOpacity={0.4} stroke="var(--color-fee)" />
    <Area dataKey="usage" type="natural" stackId="a" fill="var(--color-usage)" fillOpacity={0.4} stroke="var(--color-usage)" />
  </AreaChart>
</ChartContainer>`,
    Preview: AreaDemo,
  },
  "chart-pie": {
    title: { en: "Donut chart", zh: "環圈圖" },
    code: `const pieData = [
  { outcome: "completed", count: 62, fill: "var(--color-completed)" },
  { outcome: "stopped", count: 12, fill: "var(--color-stopped)" },
  { outcome: "failed", count: 6, fill: "var(--color-failed)" },
]

<ChartContainer config={pieConfig} className="mx-auto aspect-square h-[260px]">
  <PieChart>
    <ChartTooltip content={<ChartTooltipContent hideLabel nameKey="outcome" />} />
    <Pie data={pieData} dataKey="count" nameKey="outcome" innerRadius={62} outerRadius={96} strokeWidth={4} />
  </PieChart>
</ChartContainer>`,
    Preview: PieDemo,
  },
};
