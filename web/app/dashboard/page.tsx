import { getJson } from "@/app/lib/api"
import { Badge } from "@/components/ui/badge"
import { buttonVariants } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { CoachSummary, type CoachSummaryData } from "@/components/coach-summary"
import { cn } from "@/lib/utils"
import Link from "next/link"

interface Me { display_name: string | null; username: string; profile_public: boolean }
interface DashboardStats { human_ratio_permille: number; agent_ratio_permille: number; struggle_episodes: number; mean_pesdo: number | null }
interface Episode { id: string; started_at: string; human_seconds: number; agent_seconds: number; attempts: number; resolved: boolean; credited_seconds: number }
interface Project { id: number; name: string; total_seconds: number; human_seconds: number; agent_seconds: number; last_active: string | null }
interface DashboardInsights { most_used_harness: string | null; primary_language: string | null; most_active_day: string | null; pesdo_score_permille: number | null }

// Optional during server rollout. Existing episode/project data remains useful
// if an older server does not expose the snapshot yet.
interface DashboardSnapshot {
  last_sync_at?: string | null
  sync_status?: "synced" | "pending" | "failed" | string | null
  total_human_seconds?: number | null
  total_agent_seconds?: number | null
  total_inactive_seconds?: number | null
  input_tokens?: number | null
  output_tokens?: number | null
  feedback?: Array<{ recorded_at: string; feedback: string }> | null
}

function formatDuration(seconds: number): string {
  const value = Math.max(0, Math.floor(seconds))
  const hours = Math.floor(value / 3600)
  const minutes = Math.floor((value % 3600) / 60)
  return hours > 0 ? `${hours}h ${minutes}m` : `${minutes}m`
}

function formatTokens(tokens: number | null | undefined): string {
  if (tokens == null) return "—"
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}m`
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`
  return String(tokens)
}

function formatDate(value: string | null | undefined): string {
  if (!value) return "Not synced yet"
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? "Recently" : date.toLocaleString()
}

function percentage(value: number, total: number): number {
  return total > 0 ? Math.round((value / total) * 100) : 0
}

export default async function DashboardPage() {
  const [me, stats, episodes, projects, insights, coach, snapshot] = await Promise.all([
    getJson<Me>("/api/v1/me"),
    getJson<DashboardStats>("/api/v1/dashboard/stats"),
    getJson<Episode[]>("/api/v1/episodes"),
    getJson<Project[]>("/api/v1/projects"),
    getJson<DashboardInsights>("/api/v1/dashboard/insights"),
    getJson<CoachSummaryData>("/api/v1/coach"),
    getJson<DashboardSnapshot>("/api/v1/dashboard/snapshot"),
  ])

  if (!me || !stats) return <section className="state-card"><p className="eyebrow">Overview</p><h1>We couldn’t load your workspace.</h1><p>Your information remains protected. Please try again in a moment.</p><a className={cn(buttonVariants({ variant: "outline" }))} href="/dashboard">Refresh overview</a></section>

  const allEpisodes = episodes ?? []
  const allProjects = projects ?? []
  const totalCreditedSeconds = allEpisodes.reduce((sum, episode) => sum + episode.credited_seconds, 0)
  const recordedHumanSeconds = allEpisodes.reduce((sum, episode) => sum + episode.human_seconds, 0)
  const recordedAgentSeconds = allEpisodes.reduce((sum, episode) => sum + episode.agent_seconds, 0)
  const humanSeconds = snapshot?.total_human_seconds ?? recordedHumanSeconds
  const agentSeconds = snapshot?.total_agent_seconds ?? recordedAgentSeconds
  const activeSeconds = humanSeconds + agentSeconds
  const humanPercent = snapshot?.total_human_seconds != null ? percentage(humanSeconds, activeSeconds) : Math.round(stats.human_ratio_permille / 10)
  const agentPercent = snapshot?.total_agent_seconds != null ? percentage(agentSeconds, activeSeconds) : Math.round(stats.agent_ratio_permille / 10)
  const feedback = (snapshot?.feedback ?? []).slice(0, 3)
  const name = me.display_name ?? me.username
  const syncLabel = snapshot?.sync_status === "failed" ? "Last background sync needs attention" : snapshot?.last_sync_at ? `Synced ${formatDate(snapshot.last_sync_at)}` : "Waiting for first background sync"

  return <section className="dashboard">
    <div className="dashboard-heading">
      <div><p className="eyebrow">Workspace overview</p><h1>{name}, here’s your<br /><em>AI build activity.</em></h1><p className="heading-copy">Locally derived time, collaboration signals, and project progress. Prompt and tool text stay on your device.</p></div>
      <div className="dashboard-actions"><Badge variant={snapshot?.sync_status === "failed" ? "destructive" : "secondary"}>{syncLabel}</Badge>{me.profile_public && <a className={cn(buttonVariants({ variant: "outline", size: "sm" }), "profile-link")} href={`/profile/${encodeURIComponent(me.username)}`}>Public profile</a>}</div>
    </div>

    <div className="dashboard-metrics" aria-label="Activity summary">
      <Card><CardHeader><CardTitle>Credited build time</CardTitle></CardHeader><CardContent><strong>{formatDuration(totalCreditedSeconds)}</strong><span>{allProjects.length} tracked project{allProjects.length === 1 ? "" : "s"}</span></CardContent></Card>
      <Card><CardHeader><CardTitle>Human / AI activity</CardTitle></CardHeader><CardContent><strong>{humanPercent}% <small>human</small></strong><span>{formatDuration(humanSeconds)} human · {formatDuration(agentSeconds)} agent</span></CardContent></Card>
      <Card><CardHeader><CardTitle>Model tokens</CardTitle></CardHeader><CardContent><strong>{formatTokens((snapshot?.input_tokens ?? 0) + (snapshot?.output_tokens ?? 0))}</strong><span>{snapshot ? `${formatTokens(snapshot.input_tokens)} in · ${formatTokens(snapshot.output_tokens)} out` : "Available after automatic sync"}</span></CardContent></Card>
      <Card><CardHeader><CardTitle>Current PESDO</CardTitle></CardHeader><CardContent><strong>{insights?.pesdo_score_permille ?? stats.mean_pesdo ?? "—"}<small> / 1000</small></strong><span>{stats.mean_pesdo != null ? "Derived from recent episodes" : "No scored episodes yet"}</span></CardContent></Card>
    </div>

    <div className="dashboard-grid">
      <section className="dashboard-section" aria-labelledby="recent-heading">
        <div className="section-heading"><div><h2 id="recent-heading">Recent collaboration</h2><p>Episode outcomes and attributable work time from your local tracker.</p></div><a href="/struggles">See all episodes</a></div>
        <div className="activity-list">{allEpisodes.slice(0, 4).length === 0 ? <p className="empty">Record a session with the CLI and this space will fill in automatically.</p> : allEpisodes.slice(0, 4).map((episode) => <article className="activity-row" key={episode.id}><div><b>{episode.resolved ? "Resolved collaboration episode" : "Collaboration episode in progress"}</b><span>{formatDate(episode.started_at)} · {episode.attempts} pass{episode.attempts === 1 ? "" : "es"}</span></div><div><span>{formatDuration(episode.credited_seconds)} credited</span><Badge variant="secondary">{episode.resolved ? "Resolved" : "Review next"}</Badge></div></article>)}</div>
      </section>
      <aside className="dashboard-section workflow-panel" aria-labelledby="workflow-heading">
        <div className="section-heading"><div><h2 id="workflow-heading">Workflow signal</h2><p>Collected automatically, with no prompt text.</p></div></div>
        <dl className="signal-list"><div><dt>Primary harness</dt><dd>{insights?.most_used_harness ?? "—"}</dd></div><div><dt>Primary language</dt><dd>{insights?.primary_language ?? "—"}</dd></div><div><dt>Most active day</dt><dd>{insights?.most_active_day ?? "—"}</dd></div><div><dt>Extra-pass episodes</dt><dd>{stats.struggle_episodes}</dd></div><div><dt>Human activity</dt><dd>{humanPercent}%</dd></div><div><dt>AI activity</dt><dd>{agentPercent}%</dd></div></dl>
        {snapshot?.total_inactive_seconds != null && <p className="panel-note">{formatDuration(snapshot.total_inactive_seconds)} outside active collaboration intervals stays separate from credited time.</p>}
      </aside>
    </div>

    <section className="dashboard-section score-section" aria-labelledby="score-heading">
      <div className="section-heading"><div><h2 id="score-heading">How the score is built</h2><p>PESDO combines presence, evidence, information gain, and deliberation. Submission eligibility stays separate.</p></div></div>
      <div className="score-bars"><div><span>Presence</span><b>{humanPercent}%</b><i><em style={{ width: `${humanPercent}%` }} /></i></div><div><span>Evidence</span><b>{allEpisodes.length} episodes</b><i><em style={{ width: `${Math.min(100, allEpisodes.length * 10)}%` }} /></i></div><div><span>Iteration</span><b>{stats.struggle_episodes} extra-pass</b><i><em style={{ width: `${Math.min(100, stats.struggle_episodes * 20)}%` }} /></i></div></div>
      <p className="panel-note">These are derived signals, not judgments about you. The raw conversation remains local.</p>
    </section>

    <section className="dashboard-section projects-section" aria-labelledby="projects-heading">
      <div className="section-heading"><div><h2 id="projects-heading">Tracked projects</h2><p>Open a project for its daily rollups, time split, and recorded episodes.</p></div><a href="/projects">All projects</a></div>
      {allProjects.length === 0 ? <p className="empty">Your projects will appear here after the tracker syncs activity.</p> : <div className="dashboard-project-list">{allProjects.slice(0, 4).map((project) => <Link href={`/projects/${project.id}`} key={project.id}><span><b>{project.name}</b><small>{project.last_active ? `Last active ${formatDate(project.last_active)}` : "No activity timestamp yet"}</small></span><strong>{formatDuration(project.total_seconds)}</strong></Link>)}</div>}
    </section>

    {feedback.length > 0 && <section className="dashboard-section feedback-section" aria-labelledby="feedback-heading"><div className="section-heading"><div><h2 id="feedback-heading">Agent feedback</h2><p>Short feedback captured by the tracker after every tenth turn.</p></div></div><div className="feedback-list">{feedback.map((item, index) => <article key={`${item.recorded_at}-${index}`}><time>{formatDate(item.recorded_at)}</time><p>{item.feedback}</p></article>)}</div></section>}
    <CoachSummary coach={coach} />
  </section>
}
