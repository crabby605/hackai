import { getJson } from "@/app/lib/api"
import { EvidenceBundleList, type EvidenceBundleSummary } from "@/components/evidence-bundle-list"

export default async function EvidencePage() {
  let bundles = await getJson<EvidenceBundleSummary[]>("/api/v1/evidence-bundles")
  let preview = false
  if (process.env.NODE_ENV === "development" && !bundles) {
    preview = true
    const now = new Date().toISOString()
    bundles = [
      { bundle_id: "preview-audit-20261125", processor_version: "1.0.0", processor_hash: "3d1b09d7d7b3a4c648328e754bca881b", formula_version: "pesdo-v1", generated_at: now, received_at: now, event_count: 184, preview_approved: true },
      { bundle_id: "preview-audit-20261118", processor_version: "1.0.0", processor_hash: "ec1b805f38379f24e0ff5070d50a061b", formula_version: "pesdo-v1", generated_at: "2026-11-18T10:30:00Z", received_at: "2026-11-18T10:31:00Z", event_count: 92, preview_approved: true },
    ]
  }

  if (!bundles) {
    return <section className="state-card"><p className="eyebrow">Evidence</p><h1>Evidence summaries are unavailable.</h1><p>Please try again in a moment.</p></section>
  }

  return <section className="data-page">
    <div className="page-heading"><div><p className="eyebrow">Opt-in evidence</p><h1>What was<br /><em>shared.</em></h1><p className="heading-copy">These are redacted, derived event summaries from approved local exports. Prompts, code, file paths, and tool output are never displayed here.</p></div></div>
    {bundles.length === 0 ? <p className="empty">No opt-in evidence bundles have been shared.</p> : <EvidenceBundleList bundles={bundles} preview={preview} />}
  </section>
}
