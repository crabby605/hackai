import { getJson } from "@/app/lib/api"
import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { CoachSummary, type CoachSummaryData } from "@/components/coach-summary"

interface Episode {
  id: string
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
  let [episodes, coach] = await Promise.all([
    getJson<Episode[]>("/api/v1/episodes"),
    getJson<CoachSummaryData>("/api/v1/coach"),
  ])
  if (process.env.NODE_ENV === "development" && !episodes) {
    episodes = [
      { id: "demo-1", started_at: "2026-11-25 13:30", ended_at: "2026-11-25 14:50", attempts: 2, resolved: true, credited_seconds: 4_800, human_edit_seconds: 2_820, agent_edit_seconds: 1_980 },
      { id: "demo-2", started_at: "2026-11-24 10:20", ended_at: "2026-11-24 11:45", attempts: 3, resolved: true, credited_seconds: 5_100, human_edit_seconds: 2_760, agent_edit_seconds: 2_340 },
      { id: "demo-3", started_at: "2026-11-22 12:10", ended_at: "2026-11-22 13:20", attempts: 2, resolved: false, credited_seconds: 4_200, human_edit_seconds: 2_520, agent_edit_seconds: 1_680 },
      { id: "demo-4", started_at: "2026-11-21 10:30", ended_at: "2026-11-21 11:15", attempts: 1, resolved: true, credited_seconds: 2_700, human_edit_seconds: 540, agent_edit_seconds: 2_160 },
      { id: "demo-5", started_at: "2026-11-19 15:10", ended_at: "2026-11-19 16:45", attempts: 4, resolved: true, credited_seconds: 5_700, human_edit_seconds: 3_180, agent_edit_seconds: 2_520 },
    ]
  }
  if (!episodes) return <section className="state-card"><p className="eyebrow">Episodes</p><h1>Your episode history is temporarily unavailable.</h1><p>Please try again in a moment.</p><a className={cn(buttonVariants({ variant: "outline" }))} href="/struggles">Try again</a></section>
  const credited = episodes.reduce((total, episode) => total + episode.credited_seconds, 0)
  return <section className="data-page"><div className="page-heading"><div><p className="eyebrow">Recorded episodes</p><h1>Review your<br /><em>work sessions.</em></h1><p className="heading-copy">Open a session to see practical, per-turn improvement guidance. Prompt text is never shown.</p></div></div><div className="summary-line"><span className="summary-number">{episodes.length}</span> episode{episodes.length === 1 ? "" : "s"} · <span className="summary-number">{duration(credited)}</span> credited time</div><CoachSummary coach={coach} /><div className="data-list">{episodes.length === 0 ? <p className="empty">Recorded episodes will appear here after your next sync.</p> : episodes.map((episode) => <a href={`/struggles/${encodeURIComponent(episode.id)}`} key={episode.id} className="list-item"><div><div className="item-title">{episode.resolved ? "Resolved" : "Unresolved"} · {episode.attempts} attempt{episode.attempts === 1 ? "" : "s"}</div><div className="item-meta">Started {episode.started_at} · ended {episode.ended_at}</div></div><div><div className="item-value">{duration(episode.credited_seconds)}</div><div className="item-detail">Open improvement review →</div></div></a>)}</div></section>
}
