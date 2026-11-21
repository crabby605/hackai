import { Card, CardContent } from "@/components/ui/card"

export default function HealthPage() {
  return <main className="page-content"><p className="eyebrow">Health check</p><h1>Service <em>reachable.</em></h1><Card style={{ marginTop: "2rem" }}><CardContent><strong>ok</strong><p className="muted">For component status, uptime, and latency, see <a href="/status">the status page</a>.</p></CardContent></Card></main>
}
