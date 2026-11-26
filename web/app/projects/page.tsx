import { getJson } from "@/app/lib/api"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select"
import Link from "next/link"

interface ProjectSummary {
  id: number
  name: string
  total_seconds: number
  human_seconds: number
  agent_seconds: number
  last_active: string | null
  avg_pesdo_score_permille?: number | null
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

export default async function ProjectsPage({ searchParams }: { searchParams: Promise<{ sort?: string }> }) {
  const { sort = "time_desc" } = await searchParams
  let viewer = await getJson<{ username: string }>("/api/v1/me")
  let projects = await getJson<ProjectSummary[]>(`/api/v1/projects?sort=${encodeURIComponent(sort)}`)
  if (process.env.NODE_ENV === "development" && (!viewer || !projects)) {
    viewer = { username: "vihaan" }
    projects = [
      { id: 1, name: "ironbind", total_seconds: 51_780, human_seconds: 31_920, agent_seconds: 19_860, last_active: new Date().toISOString(), avg_pesdo_score_permille: 782 },
      { id: 2, name: "hackai", total_seconds: 42_420, human_seconds: 24_600, agent_seconds: 17_820, last_active: new Date(Date.now() - 86_400_000).toISOString(), avg_pesdo_score_permille: 814 },
      { id: 3, name: "relay", total_seconds: 28_560, human_seconds: 17_820, agent_seconds: 10_740, last_active: new Date(Date.now() - 2 * 86_400_000).toISOString(), avg_pesdo_score_permille: 739 },
      { id: 4, name: "canvas-kit", total_seconds: 18_900, human_seconds: 12_180, agent_seconds: 6_720, last_active: new Date(Date.now() - 4 * 86_400_000).toISOString(), avg_pesdo_score_permille: 761 },
      { id: 5, name: "papertrail", total_seconds: 12_240, human_seconds: 6_660, agent_seconds: 5_580, last_active: new Date(Date.now() - 7 * 86_400_000).toISOString(), avg_pesdo_score_permille: 698 },
      { id: 6, name: "weatherly", total_seconds: 7_320, human_seconds: 4_920, agent_seconds: 2_400, last_active: new Date(Date.now() - 10 * 86_400_000).toISOString(), avg_pesdo_score_permille: 755 },
    ]
  }
  if (!viewer) {
    return (
      <section className="state-card">
        <p className="eyebrow">Private by default</p>
        <h1 style={{ fontSize: "1.75rem", letterSpacing: "-0.04em", margin: "0 0 1rem" }}>
          Sign in to see your projects.
        </h1>
        <Button size="lg" render={<a href="/login" />}>Sign in <span>→</span></Button>
      </section>
    )
  }

  if (!projects) {
    return (
      <section className="state-card">
        <p className="eyebrow">Projects unavailable</p>
        <h1>Your data is safe.</h1>
        <p>Your project summary is temporarily unavailable. Please try again in a moment.</p>
        <Button variant="outline" render={<a href="/projects" />}>Try again</Button>
      </section>
    )
  }

  const totalSeconds = projects.reduce((sum, p) => sum + p.total_seconds, 0)

  return (
    <section>
      <p className="eyebrow">My Projects</p>
      <h1 style={{ fontSize: "1.75rem", letterSpacing: "-0.04em", margin: "0 0 1.5rem" }}>
        Your tracked projects.
      </h1>

      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <span className="text-sm text-muted-foreground">
          {formatDuration(totalSeconds)} total across {projects.length} project(s)
        </span>
        <form action="/projects" method="get" className="flex items-center gap-2">
          <label htmlFor="project-sort" className="text-sm text-muted-foreground">Sort</label>
          <NativeSelect id="project-sort" name="sort" defaultValue={sort} size="sm">
            <NativeSelectOption value="time_desc">Most time (high → low)</NativeSelectOption>
            <NativeSelectOption value="time_asc">Least time (low → high)</NativeSelectOption>
            <NativeSelectOption value="last_worked_desc">Last worked (newest first)</NativeSelectOption>
            <NativeSelectOption value="last_worked_asc">Last worked (oldest first)</NativeSelectOption>
            <NativeSelectOption value="name_asc">Name (A–Z)</NativeSelectOption>
            <NativeSelectOption value="name_desc">Name (Z–A)</NativeSelectOption>
          </NativeSelect>
          <Button type="submit" variant="outline" size="sm">Apply</Button>
        </form>
      </div>

      {projects.length === 0 ? (
        <p className="empty">Your projects will appear here as activity is recorded.</p>
      ) : (
        <div className="space-y-2">{projects.map((p) => {
          const humanPct = p.total_seconds > 0 ? (p.human_seconds / p.total_seconds) * 100 : 0
          const agentPct = p.total_seconds > 0 ? (p.agent_seconds / p.total_seconds) * 100 : 0
          return (
            <Link href={`/projects/${p.id}`} key={p.id} className="no-underline block">
              <Card size="sm" className="transition-colors hover:bg-muted/50">
                <CardContent className="flex items-center justify-between gap-4">
                  <div>
                    <div className="font-semibold">{p.name}</div>
                    <div className="text-xs text-muted-foreground">Last active: {formatDate(p.last_active)}</div>
                  </div>
                  <div className="text-right">
                    {p.avg_pesdo_score_permille != null && <Badge variant="outline">PESDO {p.avg_pesdo_score_permille}</Badge>}
                    <div className="mt-1 font-semibold">{formatDuration(p.total_seconds)}</div>
                    <div className="text-xs text-muted-foreground">{humanPct.toFixed(0)}% human / {agentPct.toFixed(0)}% agent</div>
                  </div>
                </CardContent>
              </Card>
            </Link>
          )
        })}</div>
      )}
    </section>
  )
}
