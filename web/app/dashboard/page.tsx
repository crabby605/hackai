import { getJson } from "@/app/lib/api"
import { redirect } from "next/navigation"
import { Badge } from "@/components/ui/badge"
import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

interface Me { is_admin: boolean; display_name: string | null; username: string; profile_public: boolean; leaderboard_opt_out: boolean }
interface DashboardStats { hours_per_day: number; human_ratio_permille: number; agent_ratio_permille: number; struggle_episodes: number }
interface Episode { started_at: string; ended_at: string; attempts: number; resolved: boolean; credited_seconds: number }
interface Project { id: string; name: string; total_seconds: number }
interface DashboardInsights { most_used_harness: string | null; primary_language: string | null; most_active_day: string | null; pesdo_score_permille: number | null; outcome_factor_permille: number }

export default async function DashboardPage() {
  const [me, stats, episodes, projects, insights] = await Promise.all([
    getJson<Me>("/api/v1/me"),
    getJson<DashboardStats>("/api/v1/dashboard/stats"),
    getJson<Episode[]>("/api/v1/episodes"),
    getJson<Project[]>("/api/v1/projects"),
    getJson<DashboardInsights>("/api/v1/dashboard/insights"),
  ])

  if (!me || !stats) return (
    <section className="state-card">
      <p className="eyebrow">Overview</p>
      <h1>We couldn’t load your workspace.</h1>
      <p>Your information remains protected. Please try again in a moment.</p>
      <a className={cn(buttonVariants({ variant: "outline" }))} href="/dashboard">Refresh overview</a>
    </section>
  )

  const human = (stats.human_ratio_permille / 10).toFixed(1)
  const agent = (stats.agent_ratio_permille / 10).toFixed(1)
  const pesdoScore = insights?.pesdo_score_permille === null || insights?.pesdo_score_permille === undefined ? "—" : `${(insights.pesdo_score_permille / 10).toFixed(1)}%`
  const name = me.display_name ?? me.username

  const formatDuration = (seconds: number) => {
    const minutes = Math.floor(seconds / 60)
    return minutes >= 60 ? `${Math.floor(minutes / 60)}h ${minutes % 60}m` : `${minutes}m`
  }
  const recentEpisodes = episodes?.slice(0, 3) ?? []
  const unresolvedEpisodes = episodes?.filter((episode) => !episode.resolved) ?? []
  const repeatedEpisodes = episodes?.filter((episode) => episode.attempts > 1) ?? []

  return (
    <section className="dashboard">
      <div className="dashboard-heading">
        <div>
          <p className="eyebrow">AI collaboration · today</p>
          <h1>Understand how<br />you <em>build with AI.</em></h1>
          <p className="heading-copy">See where AI is accelerating your work, where sessions get stuck, and what to improve next.</p>
        </div>
        {me.profile_public && <a className={cn(buttonVariants({ variant: "outline", size: "sm" }), "profile-link")} href={`/profile/${encodeURIComponent(me.username)}`}>View public profile</a>}
      </div>

      <div className="metrics-strip">
        <div><span>Focused build time</span><strong>{stats.hours_per_day}h</strong><small>{projects ? `${projects.length} active project${projects.length === 1 ? "" : "s"}` : "Project data unavailable"}</small></div>
        <div><span>PESDO score</span><strong>{pesdoScore}</strong><small>{insights?.pesdo_score_permille === null ? "Sync scored sessions to calculate" : "Weighted credit ÷ human-present time"}</small></div>
        <div><span>Most active day</span><strong>{insights?.most_active_day ?? "—"}</strong><small>{insights?.most_active_day ? "By credited build time" : "No tracked sessions yet"}</small></div>
      </div>

      <div className="overview-columns">
        <section className="recent-struggles">
          <div className="section-heading"><div><h2>Recent collaboration signals</h2><p>Where your workflow needed extra passes</p></div><a href="/struggles">Review all sessions</a></div>
          <div className="activity-list">
            {!episodes ? <p className="empty">Recent activity is temporarily unavailable.</p> : recentEpisodes.length === 0 ? <p className="empty">No collaboration signals recorded yet.</p> : recentEpisodes.map((episode) => <div className="activity-row" key={`${episode.started_at}-${episode.ended_at}`}><div><b>{episode.resolved ? "Completed after iteration" : "Still in progress"}</b><span>{new Date(episode.started_at).toLocaleString()} · {episode.attempts} pass{episode.attempts === 1 ? "" : "es"}</span></div><div><span>{formatDuration(episode.credited_seconds)}</span><Badge variant="secondary">{episode.resolved ? "Learned from" : "Review next"}</Badge></div></div>)}
          </div>
        </section>
        <aside className="profile-status">
          <h2>AI workflow snapshot</h2><p>A practical read on this workspace</p>
          <div className="profile-status-line"><span>Most used editor</span><b>{insights?.most_used_harness ?? "—"}</b></div>
          <div className="profile-status-line"><span>Primary language</span><b>{insights?.primary_language ?? "—"}</b></div>
          <div className="profile-status-line"><span>AI-assisted time</span><b>{agent}%</b></div>
          <div className="profile-status-line"><span>Sessions resolved</span><b>{Math.max(0, (episodes?.length ?? 0) - unresolvedEpisodes.length)} / {episodes?.length ?? 0}</b></div>
          <div className="profile-status-line"><span>Repeat-pass sessions</span><b>{repeatedEpisodes.length}</b></div>
          <div className="profile-status-line"><span>Outcome factor</span><b>{insights?.outcome_factor_permille === 1000 ? "Pending · 100%" : "—"}</b></div>
          <a href="/struggles">Find improvement opportunities</a>
        </aside>
      </div>
    </section>
  )
}
