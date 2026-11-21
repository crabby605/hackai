import { getJson } from "@/app/lib/api"
import { buttonVariants } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { cn } from "@/lib/utils"

interface ProjectSummary {
  id: string
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

export default async function ProjectsPage() {
  const viewer = await getJson<{ username: string }>("/api/v1/me")
  const projects = await getJson<ProjectSummary[]>("/api/v1/projects")
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

      <div className="mb-4 text-sm text-muted-foreground">
        {formatDuration(totalSeconds)} total across {projects.length} project(s)
      </div>

      {projects.length === 0 ? (
        <p className="empty">
          Your projects will appear here as activity is recorded.
        </p>
      ) : (
        projects.map((p) => {
          const humanPct = p.total_seconds > 0 ? (p.human_seconds / p.total_seconds) * 100 : 0
          const agentPct = p.total_seconds > 0 ? (p.agent_seconds / p.total_seconds) * 100 : 0
          return (
            <Card key={p.id} size="sm">
              <CardContent className="flex items-center justify-between gap-4">
              <div>
                <div className="font-semibold">{p.name}</div>
                <div className="text-xs text-muted-foreground">
                  Last active: {formatDate(p.last_active)}
                </div>
              </div>
              <div className="text-right">
                <div className="font-semibold">{formatDuration(p.total_seconds)}</div>
                <div className="text-xs text-muted-foreground">
                  {humanPct.toFixed(0)}% human / {agentPct.toFixed(0)}% agent
                </div>
              </div>
              </CardContent>
            </Card>
          )
        })
      )}
    </section>
  )
}
