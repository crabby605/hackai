import { getJson } from "@/app/lib/api"
import { Badge } from "@/components/ui/badge"
import { buttonVariants } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { PesdoChart } from "@/components/pesdo-chart"
import { ProjectSettings } from "@/components/project-settings"
import { Separator } from "@/components/ui/separator"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { cn } from "@/lib/utils"
import Link from "next/link"
import { notFound } from "next/navigation"

interface ProjectDetail {
  id: string
  name: string
  total_seconds: number
  human_seconds: number
  agent_seconds: number
  last_active: string | null
  episodes: number
  rollups: { day: string; total_seconds: number; human_seconds: number; agent_seconds: number }[]
  git_url: string | null
}

interface EpisodeSummary {
  id: string
  started_at: string
  ended_at: string
  duration: number
  human_seconds: number
  agent_seconds: number
  credited_seconds: number
  resolved: boolean
  attempts: number
  pesdo_score_permille: number | null
}

function formatDate(value: string | null): string {
  return value ? new Date(value).toLocaleString() : "—"
}

function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  return h > 0 ? `${h}h ${m}m` : `${m}m`
}

function scoreDelta(current: number, previous: number | undefined): string {
  if (previous == null) return "First score"
  const delta = current - previous
  return delta === 0 ? "No change" : `${delta > 0 ? "+" : ""}${delta}`
}

function PesdoHistory({ episodes }: { episodes: EpisodeSummary[] }) {
  const points = [...episodes].reverse().filter((episode): episode is EpisodeSummary & { pesdo_score_permille: number } => episode.pesdo_score_permille != null)
  if (points.length === 0) return <Card><CardHeader><CardTitle>PESDO score</CardTitle><CardDescription>A score trend will appear after this project has scored episodes.</CardDescription></CardHeader></Card>

  const chartData = points.map((point, index) => ({
    episode: `#${index + 1}`,
    score: point.pesdo_score_permille,
    recordedAt: formatDate(point.started_at),
  }))
  const latest = points.at(-1)?.pesdo_score_permille ?? 0
  const previous = points.at(-2)?.pesdo_score_permille
  const average = Math.round(points.reduce((total, point) => total + point.pesdo_score_permille, 0) / points.length)

  return <Card className="pesdo-history" aria-labelledby="project-pesdo-heading">
    <CardHeader><div><CardTitle id="project-pesdo-heading">Project PESDO score</CardTitle><CardDescription>Average across this project’s scored episodes.</CardDescription></div><div className="score-readout"><strong>{average}</strong><Badge variant="outline">Latest {latest} · {scoreDelta(latest, previous)}</Badge></div></CardHeader>
    <CardContent><PesdoChart data={chartData} />
    <div className="score-direction"><span>Earlier</span><span>Latest episode</span></div></CardContent>
  </Card>
}

export default async function ProjectPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params
  let [project, episodes] = await Promise.all([
    getJson<ProjectDetail>(`/api/v1/projects/${encodeURIComponent(id)}/detail`),
    getJson<EpisodeSummary[]>(`/api/v1/projects/${encodeURIComponent(id)}/episodes`),
  ])

  if (process.env.NODE_ENV === "development" && !project) {
    project = { id, name: "ironbind", total_seconds: 14_460, human_seconds: 8_640, agent_seconds: 5_820, last_active: new Date().toISOString(), episodes: 4, git_url: "https://github.com/hackclub/ironbind", rollups: [{ day: "2026-11-22", total_seconds: 3_840, human_seconds: 2_520, agent_seconds: 1_320 }, { day: "2026-11-24", total_seconds: 4_920, human_seconds: 2_760, agent_seconds: 2_160 }, { day: "2026-11-25", total_seconds: 5_700, human_seconds: 3_360, agent_seconds: 2_340 }] }
    episodes = [
      { id: "preview-4", started_at: "2026-11-25T13:30:00Z", ended_at: "2026-11-25T14:50:00Z", duration: 4_800, human_seconds: 2_820, agent_seconds: 1_980, credited_seconds: 4_800, resolved: true, attempts: 2, pesdo_score_permille: 824 },
      { id: "preview-3", started_at: "2026-11-24T10:20:00Z", ended_at: "2026-11-24T11:45:00Z", duration: 5_100, human_seconds: 2_760, agent_seconds: 2_340, credited_seconds: 5_100, resolved: true, attempts: 3, pesdo_score_permille: 771 },
      { id: "preview-2", started_at: "2026-11-22T12:10:00Z", ended_at: "2026-11-22T13:20:00Z", duration: 4_200, human_seconds: 2_520, agent_seconds: 1_680, credited_seconds: 4_200, resolved: false, attempts: 2, pesdo_score_permille: 694 },
      { id: "preview-1", started_at: "2026-11-21T10:30:00Z", ended_at: "2026-11-21T11:15:00Z", duration: 2_700, human_seconds: 540, agent_seconds: 2_160, credited_seconds: 2_700, resolved: true, attempts: 1, pesdo_score_permille: 736 },
    ]
  }

  if (!project) notFound()
  const recordedEpisodes = episodes ?? []
  const humanPct = project.total_seconds > 0 ? Math.round((project.human_seconds / project.total_seconds) * 100) : 0
  const agentPct = 100 - humanPct

  return <section className="project-detail">
    <Link href="/projects" className={cn(buttonVariants({ variant: "ghost", size: "sm" }), "project-back")}>← Projects</Link>
    <header className="project-header"><div><p className="eyebrow">Project</p><h1>{project.name}</h1><p>{project.episodes} sessions · Last active {formatDate(project.last_active)}</p></div><div className="project-header-actions"><ProjectSettings projectId={project.id} initialName={project.name} initialGitUrl={project.git_url} /><strong>{formatDuration(project.total_seconds)}<small> total tracked time</small></strong></div></header>
    <PesdoHistory episodes={recordedEpisodes} />
    <div className="project-stats" aria-label="Project activity summary"><Card><CardHeader><CardDescription>Human time</CardDescription><CardTitle>{formatDuration(project.human_seconds)}</CardTitle></CardHeader><CardContent>{humanPct}% of active time</CardContent></Card><Card><CardHeader><CardDescription>Agent time</CardDescription><CardTitle>{formatDuration(project.agent_seconds)}</CardTitle></CardHeader><CardContent>{agentPct}% of active time</CardContent></Card><Card><CardHeader><CardDescription>Sessions</CardDescription><CardTitle>{project.episodes}</CardTitle></CardHeader><CardContent>{recordedEpisodes.filter((episode) => episode.resolved).length} resolved</CardContent></Card></div>
    <Separator />
    <Card className="project-section" aria-labelledby="session-heading"><CardHeader><CardTitle id="session-heading">Sessions</CardTitle><CardDescription>What happened in each recorded work session: time, outcome, and score movement.</CardDescription></CardHeader><CardContent>{recordedEpisodes.length ? <div className="episode-list">{recordedEpisodes.map((episode, index) => { const older = recordedEpisodes[index + 1]?.pesdo_score_permille ?? undefined; return <article key={episode.id}><div><Badge variant={episode.resolved ? "secondary" : "outline"}>{episode.resolved ? "Resolved" : "Needs review"}</Badge><span>{formatDate(episode.started_at)} · {episode.attempts} pass{episode.attempts === 1 ? "" : "es"}</span></div><div><b>{formatDuration(episode.credited_seconds)}</b><span>{episode.pesdo_score_permille == null ? "Not scored" : `${episode.pesdo_score_permille} · ${scoreDelta(episode.pesdo_score_permille, older)}`}</span></div></article> })}</div> : <p className="empty">No sessions recorded for this project.</p>}</CardContent></Card>
    {project.rollups.length > 0 && <Card className="project-section" aria-labelledby="rollups-heading"><CardHeader><CardTitle id="rollups-heading">Daily activity</CardTitle><CardDescription>Credited time remains split between you and the agent.</CardDescription></CardHeader><CardContent><Table><TableHeader><TableRow><TableHead>Day</TableHead><TableHead>Total</TableHead><TableHead>Human</TableHead><TableHead>Agent</TableHead></TableRow></TableHeader><TableBody>{project.rollups.map((rollup) => <TableRow key={rollup.day}><TableCell>{rollup.day}</TableCell><TableCell>{formatDuration(rollup.total_seconds)}</TableCell><TableCell>{formatDuration(rollup.human_seconds)}</TableCell><TableCell>{formatDuration(rollup.agent_seconds)}</TableCell></TableRow>)}</TableBody></Table></CardContent></Card>}
  </section>
}
