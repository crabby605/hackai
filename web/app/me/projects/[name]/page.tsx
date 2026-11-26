import { getJson } from "@/app/lib/api"
import { notFound } from "next/navigation"
import { Card, CardContent } from "@/components/ui/card"
import { buttonVariants } from "@/components/ui/button"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { cn } from "@/lib/utils"
import Link from "next/link"

interface ProjectDetail {
  id: string
  name: string
  total_seconds: number
  human_seconds: number
  agent_seconds: number
  last_active: string | null
  episodes: number
  rollups: { day: string; total_seconds: number; human_seconds: number; agent_seconds: number }[]
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
}

function formatDate(value: string | null): string {
  return value ? new Date(value).toLocaleString() : "—"
}

function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  const s = Math.floor(seconds % 60)
  if (h > 0) return `${h}h ${m}m ${s}s`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

function formatEpisodeDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  const s = Math.floor(seconds % 60)
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

export default async function ProjectPage({ params }: { params: Promise<{ name: string }> }) {
  const { name } = await params
  const [project, episodes] = await Promise.all([
    getJson<ProjectDetail>(`/api/v1/projects/by-name/${encodeURIComponent(name)}/detail`),
    getJson<EpisodeSummary[]>(`/api/v1/projects/by-name/${encodeURIComponent(name)}/episodes`),
  ])

  if (!project) notFound()

  const humanPct = project.total_seconds > 0 ? (project.human_seconds / project.total_seconds) * 100 : 0
  const agentPct = project.total_seconds > 0 ? (project.agent_seconds / project.total_seconds) * 100 : 0

  return (
    <section>
      <Link href="/projects" className={cn(buttonVariants({ variant: "outline", size: "sm" }), "mb-4 inline-block")}>
        ← Back to projects
      </Link>

      <div className="mb-6">
        <p className="eyebrow">Project detail</p>
        <h1 style={{ fontSize: "1.75rem", letterSpacing: "-0.04em", margin: "0 0 0.5rem" }}>{project.name}</h1>
        <div className="text-sm text-muted-foreground">
          {project.episodes} episodes · Last active: {formatDate(project.last_active)}
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
        <Card>
          <CardContent className="pt-6">
            <div className="text-sm text-muted-foreground">Total time</div>
            <div className="text-2xl font-bold">{formatDuration(project.total_seconds)}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="text-sm text-muted-foreground">Human time</div>
            <div className="text-2xl font-bold">{formatDuration(project.human_seconds)}</div>
            <div className="text-xs text-muted-foreground">{humanPct.toFixed(1)}%</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="text-sm text-muted-foreground">Agent time</div>
            <div className="text-2xl font-bold">{formatDuration(project.agent_seconds)}</div>
            <div className="text-xs text-muted-foreground">{agentPct.toFixed(1)}%</div>
          </CardContent>
        </Card>
      </div>

      <div className="mb-6">
        <h2>Recent episodes</h2>
        {episodes?.length ? (
          <div className="space-y-2 mt-2">
            {episodes.map((ep) => (
              <Card key={ep.id} size="sm">
                <CardContent className="flex items-center justify-between gap-4">
                  <div>
                    <div className="font-semibold">
                      {ep.resolved ? "Completed" : "In progress"} · {ep.attempts} pass{ep.attempts === 1 ? "" : "es"}
                    </div>
                    <div className="text-xs text-muted-foreground">
                      {new Date(ep.started_at).toLocaleString()} · {formatEpisodeDuration(ep.duration)}
                    </div>
                  </div>
                  <div className="text-right">
                    <div className="font-semibold">{formatDuration(ep.credited_seconds)} credited</div>
                    <div className="text-xs text-muted-foreground">
                      {ep.human_seconds > 0 || ep.agent_seconds > 0
                        ? `H: ${formatDuration(ep.human_seconds)} / A: ${formatDuration(ep.agent_seconds)}`
                        : "—"}
                    </div>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground mt-2">No episodes recorded for this project.</p>
        )}
      </div>

      {project.rollups?.length && (
        <div>
          <h2>Daily rollups (last 60 days)</h2>
          <div className="mt-2">
            <Table>
              <TableHeader><TableRow>
                <TableHead>Day</TableHead>
                <TableHead>Total</TableHead>
                <TableHead>Human</TableHead>
                <TableHead>Agent</TableHead>
              </TableRow></TableHeader>
              <TableBody>
                {project.rollups.map((r) => (
                  <TableRow key={r.day}>
                    <TableCell>{r.day}</TableCell>
                    <TableCell>{formatDuration(r.total_seconds)}</TableCell>
                    <TableCell>{formatDuration(r.human_seconds)}</TableCell>
                    <TableCell>{formatDuration(r.agent_seconds)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        </div>
      )}
    </section>
  )
}
