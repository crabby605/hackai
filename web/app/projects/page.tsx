import { getJson } from "@/app/lib/api"
import { buttonVariants } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { cn } from "@/lib/utils"
import Link from "next/link"

interface ProjectSummary {
  id: number
  name: string
  total_seconds: number
  human_seconds: number
  agent_seconds: number
  last_active: string | null
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
  const viewer = await getJson<{ username: string }>("/api/v1/me")
  const projects = await getJson<ProjectSummary[]>(`/api/v1/projects?sort=${encodeURIComponent(sort)}`)
  if (!viewer) {
    return (
      <section className="state-card">
        <p className="eyebrow">Private by default</p>
        <h1 style={{ fontSize: "1.75rem", letterSpacing: "-0.04em", margin: "0 0 1rem" }}>
          Sign in to see your projects.
        </h1>
        <a className={cn(buttonVariants({ size: "lg" }))} href="/login">
          Sign in <span>→</span>
        </a>
      </section>
    )
  }

  if (!projects) {
    return (
      <section className="state-card">
        <p className="eyebrow">Projects unavailable</p>
        <h1>Your data is safe.</h1>
        <p>Your project summary is temporarily unavailable. Please try again in a moment.</p>
        <a className={cn(buttonVariants({ variant: "outline" }))} href="/projects">
          Try again
        </a>
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
          <select id="project-sort" name="sort" defaultValue={sort} className="border rounded px-2 py-1 text-sm bg-background">
            <option value="time_desc">Most time (high → low)</option>
            <option value="time_asc">Least time (low → high)</option>
            <option value="last_worked_desc">Last worked (newest first)</option>
            <option value="last_worked_asc">Last worked (oldest first)</option>
            <option value="name_asc">Name (A–Z)</option>
            <option value="name_desc">Name (Z–A)</option>
          </select>
          <button type="submit" className={cn(buttonVariants({ variant: "outline", size: "sm" }))}>Apply</button>
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
                    <div className="font-semibold">{formatDuration(p.total_seconds)}</div>
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
