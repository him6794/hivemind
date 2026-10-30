"use client";

import { useEffect, useState } from "react";
import { AnimatePresence, animate, motion, useReducedMotion } from "framer-motion";
import { Button } from "@/components/shadcn/button";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/shadcn/card";
import type { DemoEntry } from "./docs-demo-types";

// Reduced motion only shortens the transitions. Initial states stay the same on
// the server and in the browser, so the first render matches.

const cards = [
  { title: "Queued", value: "3 tasks" },
  { title: "Running", value: "12 replicas" },
  { title: "Settled", value: "38 tasks" },
];

function StaggerDemo() {
  const reduced = useReducedMotion();
  const [run, setRun] = useState(0);
  const container = { hidden: {}, show: { transition: { staggerChildren: reduced ? 0 : 0.12 } } };
  const item = {
    hidden: { opacity: 0, y: 14 },
    show: { opacity: 1, y: 0, transition: { duration: reduced ? 0 : 0.45, ease: [0.16, 1, 0.3, 1] as const } },
  };
  return (
    <div className="flex w-full max-w-xl flex-col items-center gap-4">
      <motion.div
        key={run}
        variants={container}
        initial="hidden"
        animate="show"
        className="grid w-full gap-3 sm:grid-cols-3"
      >
        {cards.map((card) => (
          <motion.div key={card.title} variants={item}>
            <Card className="h-full">
              <CardHeader>
                <CardDescription>{card.title}</CardDescription>
                <CardTitle className="text-xl">{card.value}</CardTitle>
              </CardHeader>
            </Card>
          </motion.div>
        ))}
      </motion.div>
      <Button variant="outline" size="sm" onClick={() => setRun((count) => count + 1)}>
        Replay
      </Button>
    </div>
  );
}

function CounterDemo() {
  const reduced = useReducedMotion();
  const [value, setValue] = useState(0);
  const [run, setRun] = useState(0);

  useEffect(() => {
    if (reduced) {
      setValue(1248);
      return;
    }
    setValue(0);
    const controls = animate(0, 1248, {
      duration: 1.6,
      ease: "easeOut",
      onUpdate: (latest) => setValue(Math.round(latest)),
    });
    return () => controls.stop();
  }, [run, reduced]);

  return (
    <div className="flex flex-col items-center gap-4">
      <p className="font-mono-tech text-5xl font-semibold tabular-nums">{value.toLocaleString("en-US")}</p>
      <p className="text-sm text-muted-foreground">CPT usage recorded (sample)</p>
      <Button variant="outline" size="sm" onClick={() => setRun((count) => count + 1)}>
        Replay
      </Button>
    </div>
  );
}

function SpringProgressDemo() {
  const reduced = useReducedMotion();
  const [run, setRun] = useState(0);
  return (
    <div className="w-full max-w-md space-y-4">
      <div className="flex justify-between text-sm text-muted-foreground">
        <span>Replicas reporting</span>
        <span className="tabular-nums">72%</span>
      </div>
      <div className="h-2 w-full overflow-hidden rounded-full bg-secondary">
        <motion.div
          key={run}
          className="h-full rounded-full bg-primary"
          initial={{ width: "0%" }}
          animate={{ width: "72%" }}
          transition={reduced ? { duration: 0 } : { type: "spring", stiffness: 120, damping: 22 }}
        />
      </div>
      <Button variant="outline" size="sm" onClick={() => setRun((count) => count + 1)}>
        Replay
      </Button>
    </div>
  );
}

const expandRows = [
  { id: "units", title: "Usage units", body: "One unit is one CPT of replica usage. Every statement and call adds units." },
  { id: "cap", title: "Charge cap", body: "max_cpt covers the whole task, including the platform fee and every replica." },
  { id: "retry", title: "Retries", body: "Automatic paid retries are disabled, so retry work is never charged." },
];

function ExpandDemo() {
  const reduced = useReducedMotion();
  const [open, setOpen] = useState<string | null>("units");
  return (
    <div className="w-full max-w-md space-y-2">
      {expandRows.map((row) => {
        const expanded = open === row.id;
        return (
          <motion.div key={row.id} layout className="overflow-hidden rounded-xl border border-border bg-card">
            <button
              type="button"
              aria-expanded={expanded}
              onClick={() => setOpen(expanded ? null : row.id)}
              className="flex w-full items-center justify-between px-4 py-3 text-left text-sm font-medium"
            >
              {row.title}
              <motion.span animate={{ rotate: expanded ? 45 : 0 }} className="text-lg leading-none text-muted-foreground">
                +
              </motion.span>
            </button>
            <AnimatePresence initial={false}>
              {expanded ? (
                <motion.p
                  key="body"
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: "auto", opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  transition={{ duration: reduced ? 0 : 0.25 }}
                  className="px-4 pb-4 text-sm leading-relaxed text-muted-foreground"
                >
                  {row.body}
                </motion.p>
              ) : null}
            </AnimatePresence>
          </motion.div>
        );
      })}
    </div>
  );
}

function HoverTapDemo() {
  return (
    <div className="grid w-full max-w-md gap-3 sm:grid-cols-2">
      <motion.div whileHover={{ y: -4 }} transition={{ type: "spring", stiffness: 300, damping: 20 }}>
        <Card className="h-full">
          <CardHeader>
            <CardTitle className="text-base">Lift on hover</CardTitle>
            <CardDescription>The card rises while the pointer is over it.</CardDescription>
          </CardHeader>
        </Card>
      </motion.div>
      <div className="flex items-center justify-center">
        <motion.div whileHover={{ scale: 1.04 }} whileTap={{ scale: 0.96 }}>
          <Button>Press me</Button>
        </motion.div>
      </div>
    </div>
  );
}

export const MOTION_DEMOS: Record<string, DemoEntry> = {
  "anim-stagger": {
    title: { en: "Staggered entrance", zh: "依序進場" },
    code: `const container = { hidden: {}, show: { transition: { staggerChildren: 0.12 } } }
const item = {
  hidden: { opacity: 0, y: 14 },
  show: { opacity: 1, y: 0, transition: { duration: 0.45 } },
}

<motion.div variants={container} initial="hidden" animate="show" className="grid gap-3 sm:grid-cols-3">
  {cards.map((card) => (
    <motion.div key={card.title} variants={item}>
      <Card>...</Card>
    </motion.div>
  ))}
</motion.div>`,
    Preview: StaggerDemo,
  },
  "anim-counter": {
    title: { en: "Counting up", zh: "數字累加" },
    code: `const controls = animate(0, 1248, {
  duration: 1.6,
  ease: "easeOut",
  onUpdate: (latest) => setValue(Math.round(latest)),
})

return () => controls.stop()`,
    Preview: CounterDemo,
  },
  "anim-progress": {
    title: { en: "Spring progress", zh: "彈簧進度條" },
    code: `<div className="h-2 w-full overflow-hidden rounded-full bg-secondary">
  <motion.div
    className="h-full rounded-full bg-primary"
    initial={{ width: "0%" }}
    animate={{ width: "72%" }}
    transition={{ type: "spring", stiffness: 120, damping: 22 }}
  />
</div>`,
    Preview: SpringProgressDemo,
  },
  "anim-expand": {
    title: { en: "Expanding rows", zh: "展開列" },
    code: `<motion.div layout className="overflow-hidden rounded-xl border">
  <button aria-expanded={expanded} onClick={() => setOpen(expanded ? null : row.id)}>
    {row.title}
  </button>
  <AnimatePresence initial={false}>
    {expanded ? (
      <motion.p
        key="body"
        initial={{ height: 0, opacity: 0 }}
        animate={{ height: "auto", opacity: 1 }}
        exit={{ height: 0, opacity: 0 }}
      >
        {row.body}
      </motion.p>
    ) : null}
  </AnimatePresence>
</motion.div>`,
    Preview: ExpandDemo,
  },
  "anim-hover": {
    title: { en: "Hover and press", zh: "懸停與按下" },
    code: `<motion.div whileHover={{ y: -4 }} transition={{ type: "spring", stiffness: 300, damping: 20 }}>
  <Card>...</Card>
</motion.div>

<motion.div whileHover={{ scale: 1.04 }} whileTap={{ scale: 0.96 }}>
  <Button>Press me</Button>
</motion.div>`,
    Preview: HoverTapDemo,
  },
};
