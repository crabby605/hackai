"use client"

import { useCallback, useEffect, useState } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

type Status = { status: string; version: string; uptime_seconds: number; components: Record<string, { status: string; latency_ms?: number }> }

export default function HealthPage() {
  const [status, setStatus] = useState<Status | null>(null)
  const [latency, setLatency] = useState<number | null>(null)
  const [checkedAt, setCheckedAt] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  const check = useCallback(async () => {
    const started = performance.now()
    try {
      const response = await fetch("/api/v1/health", { cache: "no-store" })
      setLatency(Math.round(performance.now() - started)); setFailed(!response.ok)
      if (response.ok) setStatus(await response.json() as Status)
      setCheckedAt(new Date().toISOString())
    } catch { setFailed(true); setLatency(Math.round(performance.now() - started)); setCheckedAt(new Date().toISOString()) }
  }, [])
  useEffect(() => { check(); const timer = window.setInterval(check, 15_000); return () => window.clearInterval(timer) }, [check])
  const names = ["beats", "web", "auth", "storage"] as const
  const overall = failed ? "degraded" : status?.status ?? "checking"
  return <main className="page-content">
    <p className="eyebrow">HackAI status</p><h1>Everything <em>in view.</em></h1>
    <p className="muted">Live status for beats, web, authentication, and storage. This page refreshes every 15 seconds.</p>
    <div className="dashboard-grid" style={{ marginTop: "2rem" }}>
      <Card><CardHeader><CardTitle>Overall</CardTitle></CardHeader><CardContent><Badge variant={overall === "operational" ? "default" : overall === "checking" ? "secondary" : "destructive"}>{overall}</Badge><p className="muted">v{status?.version ?? "—"}</p></CardContent></Card>
      <Card><CardHeader><CardTitle>Uptime</CardTitle></CardHeader><CardContent><strong>{status ? `${Math.floor(status.uptime_seconds / 3600)}h ${Math.floor(status.uptime_seconds / 60) % 60}m` : "—"}</strong><p className="muted">Current API process</p></CardContent></Card>
      <Card><CardHeader><CardTitle>Latency</CardTitle></CardHeader><CardContent><strong>{latency == null ? "—" : `${latency} ms`}</strong><p className="muted">Status request round trip</p></CardContent></Card>
    </div>
    <div className="dashboard-grid" style={{ marginTop: "1rem" }}>{names.map((name) => { const component = status?.components[name]; return <Card key={name}><CardHeader><CardTitle>{name[0].toUpperCase() + name.slice(1)}</CardTitle></CardHeader><CardContent><Badge variant={component?.status === "operational" ? "default" : component ? "destructive" : "secondary"}>{component?.status ?? "checking"}</Badge>{component?.latency_ms != null && <p className="muted">{component.latency_ms} ms</p>}</CardContent></Card> })}</div>
    <p className="muted" style={{ marginTop: "1.5rem" }}>Last checked: {checkedAt ? new Date(checkedAt).toLocaleTimeString() : "—"}</p>
  </main>
}
