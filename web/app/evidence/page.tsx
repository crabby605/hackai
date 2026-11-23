import { getJson } from "@/app/lib/api"
import { Card, CardContent } from "@/components/ui/card"

type EvidenceBundle = {
  bundle_id: string
  processor_version: string
  processor_hash: string
  formula_version: string
  generated_at: string
  received_at: string
  event_count: number
  preview_approved: boolean
}

function shortHash(value: string) {
  return value.length > 16 ? `${value.slice(0, 16)}…` : value
}

export default async function EvidencePage() {
  const bundles = await getJson<EvidenceBundle[]>("/api/v1/evidence-bundles")

  if (!bundles) {
    return <section className="state-card"><p className="eyebrow">Evidence</p><h1>Evidence summaries are unavailable.</h1><p>Please try again in a moment.</p></section>
  }

  return <section className="data-page">
    <div className="page-heading"><div><p className="eyebrow">Opt-in evidence</p><h1>What was<br /><em>shared.</em></h1><p className="heading-copy">These are redacted, derived event summaries from approved local exports. Prompts, code, file paths, and tool output are never displayed here.</p></div></div>
    {bundles.length === 0 ? <p className="empty">No opt-in evidence bundles have been shared.</p> : <div className="space-y-3">{bundles.map((bundle) => <Card key={bundle.bundle_id} size="sm"><CardContent className="grid gap-3 sm:grid-cols-[1fr_auto] sm:items-center"><div><h2 className="text-base font-semibold">{bundle.event_count} derived event{bundle.event_count === 1 ? "" : "s"}</h2><p className="mt-1 text-sm text-muted-foreground">Generated {new Date(bundle.generated_at).toLocaleString()} · received {new Date(bundle.received_at).toLocaleString()}</p><p className="mt-1 text-xs text-muted-foreground">Processor {bundle.processor_version} · formula {bundle.formula_version} · {shortHash(bundle.processor_hash)}</p></div><span className="text-sm font-medium">{bundle.preview_approved ? "Approved export" : "Recorded export"}</span></CardContent></Card>)}</div>}
  </section>
}
