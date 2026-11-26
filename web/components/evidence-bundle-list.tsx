"use client"

import { useState } from "react"
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/ui/accordion"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"

export interface EvidenceBundleSummary {
  bundle_id: string
  processor_version: string
  processor_hash: string
  formula_version: string
  generated_at: string
  received_at: string
  event_count: number
  preview_approved: boolean
}

type DerivedEvent = { event_id: string; occurred_at: string; event_kind: string; record: { dialogue?: { user_act?: string; agent_state?: string; verification?: string; ctx_evidence?: boolean; ctx_expectation?: boolean; ctx_location?: boolean; tool_errors?: number }; prompt_features?: { length_bucket?: string; anchor_count_bucket?: string; fullcaps?: boolean }; reason_codes?: string[] } }
type BundleDetail = { events: DerivedEvent[] }

const previewEvents: DerivedEvent[] = [
  { event_id: "evt-001", occurred_at: "2026-11-25T13:31:00Z", event_kind: "prompt_submit", record: { dialogue: { user_act: "specify", agent_state: "planned", verification: "unverified", ctx_evidence: true, ctx_expectation: true, ctx_location: true }, prompt_features: { length_bucket: "medium", anchor_count_bucket: "two_to_three", fullcaps: false }, reason_codes: ["ANCHOR_FOUND"] } },
  { event_id: "evt-002", occurred_at: "2026-11-25T13:52:00Z", event_kind: "tool_result", record: { dialogue: { agent_state: "edited_red", verification: "red", tool_errors: 1 }, reason_codes: ["TEST_FAILED"] } },
  { event_id: "evt-003", occurred_at: "2026-11-25T14:08:00Z", event_kind: "prompt_submit", record: { dialogue: { user_act: "report_failure", agent_state: "edited_red", verification: "red", ctx_evidence: true, ctx_expectation: true, ctx_location: false }, prompt_features: { length_bucket: "short", anchor_count_bucket: "one", fullcaps: false }, reason_codes: ["ERROR_PREVIOUSLY_OBSERVED"] } },
  { event_id: "evt-004", occurred_at: "2026-11-25T14:42:00Z", event_kind: "test_run", record: { dialogue: { agent_state: "edited_green", verification: "green", tool_errors: 0 }, reason_codes: ["TEST_PASSED", "HUMAN_EDIT_OBSERVED"] } },
]

function shortHash(value: string) { return value.length > 16 ? `${value.slice(0, 16)}…` : value }
function title(value: string | undefined) { return value ? value.replaceAll("_", " ") : "—" }

export function EvidenceBundleList({ bundles, preview = false }: { bundles: EvidenceBundleSummary[]; preview?: boolean }) {
  const [details, setDetails] = useState<Record<string, BundleDetail | "loading" | "error">>({})
  async function load(bundleId: string) {
    if (details[bundleId]) return
    if (preview) { setDetails((current) => ({ ...current, [bundleId]: { events: previewEvents } })); return }
    setDetails((current) => ({ ...current, [bundleId]: "loading" }))
    try {
      const response = await fetch(`/api/v1/evidence-bundles/${encodeURIComponent(bundleId)}`, { credentials: "include" })
      if (!response.ok) throw new Error("load failed")
      const detail = await response.json() as BundleDetail
      setDetails((current) => ({ ...current, [bundleId]: detail }))
    } catch { setDetails((current) => ({ ...current, [bundleId]: "error" })) }
  }

  return <Accordion className="space-y-3" onValueChange={(value) => { if (typeof value === "string") void load(value) }}>
    {bundles.map((bundle) => <AccordionItem key={bundle.bundle_id} value={bundle.bundle_id}><Card size="sm"><CardContent><AccordionTrigger><span><b className="block text-base">{new Date(bundle.generated_at).toLocaleString()}</b><span className="mt-1 block text-xs text-muted-foreground">{bundle.event_count} derived events · processor {bundle.processor_version} · {shortHash(bundle.processor_hash)}</span></span><Badge variant={bundle.preview_approved ? "secondary" : "outline"}>{bundle.preview_approved ? "Approved" : "Recorded"}</Badge></AccordionTrigger><AccordionContent><div className="pt-2"><p className="mb-3 text-xs text-muted-foreground">Privacy-safe transcript encoding. Prompts, code, paths, and tool output are not included.</p><Trace detail={details[bundle.bundle_id]} /></div></AccordionContent></CardContent></Card></AccordionItem>)}
  </Accordion>
}

function Trace({ detail }: { detail: BundleDetail | "loading" | "error" | undefined }) {
  if (!detail || detail === "loading") return <p className="text-sm text-muted-foreground">Loading derived trace…</p>
  if (detail === "error") return <p className="text-sm text-destructive">Couldn’t load this derived trace.</p>
  return <div className="space-y-2">{detail.events.map((event) => { const dialogue = event.record.dialogue; const prompt = event.record.prompt_features; return <div className="rounded-lg border border-border p-3" key={event.event_id}><div className="flex flex-wrap items-center gap-2"><Badge variant="outline">{title(event.event_kind)}</Badge><span className="text-xs text-muted-foreground">{new Date(event.occurred_at).toLocaleString()}</span></div>{dialogue && <div className="mt-2 flex flex-wrap gap-1.5"><Badge variant="secondary">U: {title(dialogue.user_act)}</Badge><Badge variant="secondary">A: {title(dialogue.agent_state)}</Badge><Badge variant="outline">{title(dialogue.verification)}</Badge>{dialogue.ctx_evidence && <Badge variant="outline">evidence</Badge>}{dialogue.ctx_expectation && <Badge variant="outline">expectation</Badge>}{dialogue.ctx_location && <Badge variant="outline">location</Badge>}</div>}{prompt && <p className="mt-2 text-xs text-muted-foreground">Prompt shape: {title(prompt.length_bucket)} · anchors {title(prompt.anchor_count_bucket)}{prompt.fullcaps ? " · fullcaps" : ""}</p>}{event.record.reason_codes && <p className="mt-2 text-xs text-muted-foreground">Signals: {event.record.reason_codes.map(title).join(", ")}</p>}</div> })}</div>
}
