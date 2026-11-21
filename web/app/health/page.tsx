"use client"

import { useCallback, useEffect, useState } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

type Check = { ok: boolean; latency: number; at: string }

export default function HealthPage() {
  const [checks, setChecks] = useState<Check[]>([])
  const [checking, setChecking] = useState(true)

  const check = useCallback(async () => {
    const started = performance.now()
    try {
      const response = await fetch("/healthz", { cache: "no-store" })
      const latency = Math.round(performance.now() - started)
      setChecks((current) => [...current, { ok: response.ok, latency, at: new Date().toISOString() }].slice(-60))
    } catch {
      setChecks((current) => [...current, { ok: false, latency: Math.round(performance.now() - started), at: new Date().toISOString() }].slice(-60))
    } finally {
      setChecking(false)
    }
  }, [])

  useEffect(() => {
    check()
    const timer = window.setInterval(check, 15_000)
    return () => window.clearInterval(timer)
  }, [check])

  const healthy = checks.at(-1)?.ok ?? false
  const successful = checks.filter((entry) => entry.ok).length
  const uptime = checks.length ? Math.round((successful / checks.length) * 1000) / 10 : 0
  const latestLatency = checks.at(-1)?.latency

  return (
    <main className="page-content">
      <p className="eyebrow">Service health</p>
      <h1>Health <em>at a glance.</em></h1>
      <p className="muted">A privacy-safe browser check of the HackAI web and API path. Checks run every 15 seconds while this page is open.</p>
      <div className="dashboard-grid" style={{ marginTop: "2rem" }}>
        <Card><CardHeader><CardTitle>Status</CardTitle></CardHeader><CardContent><Badge variant={healthy ? "default" : "destructive"}>{checking ? "Checking…" : healthy ? "Operational" : "Unavailable"}</Badge></CardContent></Card>
        <Card><CardHeader><CardTitle>Observed uptime</CardTitle></CardHeader><CardContent><strong>{uptime}%</strong><p className="muted">{checks.length} checks in this session</p></CardContent></Card>
        <Card><CardHeader><CardTitle>Latest latency</CardTitle></CardHeader><CardContent><strong>{latestLatency == null ? "—" : `${latestLatency} ms`}</strong><p className="muted">Round trip to /healthz</p></CardContent></Card>
      </div>
      <p className="muted" style={{ marginTop: "1.5rem" }}>Last checked: {checks.at(-1) ? new Date(checks.at(-1)!.at).toLocaleTimeString() : "—"}</p>
    </main>
  )
}
