import { getJson } from "@/app/lib/api"
import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

type SessionImprovement = {
  session_id: string
  agent_name: string
  pesdo_score_permille: number | null
  pesdo_delta: number | null
  turns: { turn: number; user_summary: string; agent_summary: string; impact: string; next_step: string }[]
}

export default async function SessionImprovementPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params
  const session = await getJson<SessionImprovement>(`/api/v1/episodes/${encodeURIComponent(id)}/improvement`)
  if (!session) return <section className="state-card"><p className="eyebrow">Session review</p><h1>This session is unavailable.</h1><p>It may not have finished syncing yet.</p><a className={cn(buttonVariants({ variant: "outline" }))} href="/struggles">Back to sessions</a></section>
  const delta = session.pesdo_delta == null ? "First scored session" : `${session.pesdo_delta >= 0 ? "+" : ""}${session.pesdo_delta}`
  return <section className="data-page"><div className="page-heading"><div><p className="eyebrow">Session improvement review</p><h1>What helped you<br /><em>move forward.</em></h1><p className="heading-copy">This is coaching based on privacy-safe, local classifications—not a transcript or a judgement of you.</p></div></div>{session.pesdo_score_permille != null && <div className="summary-line"><span className="summary-number">{session.pesdo_score_permille}</span> PESDO · Δ {delta}</div>}<div className="data-list">{session.turns.length === 0 ? <p className="empty">This session has no turn-level improvement data yet.</p> : session.turns.map((turn) => <article className="list-item" key={turn.turn}><div><div className="item-title">Step {turn.turn}</div><div className="item-meta">You: {turn.user_summary}</div><div className="item-meta">{session.agent_name}: {turn.agent_summary}</div><div className="item-meta">Impact: {turn.impact}</div></div><div className="item-detail">Next: {turn.next_step}</div></article>)}</div><a className={cn(buttonVariants({ variant: "outline" }))} href="/struggles">Back to sessions</a></section>
}
