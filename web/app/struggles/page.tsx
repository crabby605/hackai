import { getJson } from "@/app/lib/api"
import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { CoachSummary, type CoachSummaryData } from "@/components/coach-summary"

interface Episode {
  started_at: string
  ended_at: string
  attempts: number
  resolved: boolean
  credited_seconds: number
  human_edit_seconds: number
  agent_edit_seconds: number
}

function duration(seconds: number) {
  const minutes = Math.floor(seconds / 60)
  return minutes >= 60 ? `${Math.floor(minutes / 60)}h ${minutes % 60}m` : `${minutes}m`
}

export default async function StrugglesPage() {
  const [episodes, coach] = await Promise.all([
    getJson<Episode[]>("/api/v1/episodes"),
    getJson<CoachSummaryData>("/api/v1/coach"),
  ])
  if (!episodes) return <section className="state-card"><p className="eyebrow">Episodes</p><h1>Your episode history is temporarily unavailable.</h1><p>Please try again in a moment.</p><a className={cn(buttonVariants({ variant: "outline" }))} href="/struggles">Try again</a></section>
  const credited = episodes.reduce((total, episode) => total + episode.credited_seconds, 0)
  return <section className="data-page"><div className="page-heading"><div><p className="eyebrow">Recorded episodes</p><h1>Review your<br /><em>work sessions.</em></h1><p className="heading-copy">Timing, iteration, and outcome summaries from your local tracker. Prompt text is not shown here.</p></div></div><div className="summary-line"><span className="summary-number">{episodes.length}</span> episode{episodes.length === 1 ? "" : "s"} · <span className="summary-number">{duration(credited)}</span> credited time</div><CoachSummary coach={coach} /><div className="data-list">{episodes.length === 0 ? <p className="empty">Recorded episodes will appear here after your next sync.</p> : episodes.map((episode) => <div key={`${episode.started_at}-${episode.ended_at}`} className="list-item"><div><div className="item-title">{episode.resolved ? "Resolved" : "Unresolved"} · {episode.attempts} attempt{episode.attempts === 1 ? "" : "s"}</div><div className="item-meta">Started {episode.started_at} · ended {episode.ended_at}</div></div><div><div className="item-value">{duration(episode.credited_seconds)}</div><div className="item-detail">human {episode.human_edit_seconds}s · agent {episode.agent_edit_seconds}s</div></div></div>)}</div></section>
}
