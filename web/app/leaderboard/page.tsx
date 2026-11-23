import { getJson } from "@/app/lib/api"
import { Card, CardContent } from "@/components/ui/card"

interface LeaderboardEntry {
  rank: number
  username: string
  display_name: string | null
  country_code: string | null
  credited_seconds: number
  projects: number
}
interface Leaderboard { total_users: number; entries: LeaderboardEntry[] }

function formatDuration(seconds: number) {
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  return hours > 0 ? `${hours}h ${minutes}m` : `${minutes}m`
}
function flag(country: string | null) { return country && /^[A-Z]{2}$/.test(country) ? String.fromCodePoint(...[...country].map((letter) => 127397 + letter.charCodeAt(0))) : "" }

export default async function LeaderboardPage() {
  const data = await getJson<LeaderboardEntry[] | Leaderboard>("/api/v1/leaderboard")
  const entries: LeaderboardEntry[] = Array.isArray(data) ? data : (data?.entries ?? [])
  const totalUsers = Array.isArray(data) ? data.length : (data?.total_users ?? entries.length)
  return (
    <section className="data-page leaderboard-page">
      <div className="page-heading"><div><p className="eyebrow">Leaderboard</p><h1>Builders</h1><p className="heading-copy">Top 100 by public credited time · {totalUsers} participating {totalUsers === 1 ? "builder" : "builders"}.</p></div></div>
      {!data ? <Card><CardContent><h2 className="font-semibold">The leaderboard is unavailable.</h2><p className="text-muted-foreground">Please try again shortly.</p></CardContent></Card> : entries.length === 0 ? <Card><CardContent><p className="text-muted-foreground">No public activity yet. Be the first builder to share a profile.</p></CardContent></Card> : <div className="space-y-2">{entries.map((entry) => <Card key={entry.username} size="sm" className={entry.rank <= 3 ? "border-primary/40 bg-primary/5" : undefined}><CardContent><a className="flex items-center gap-4" href={`/profile/${encodeURIComponent(entry.username)}`}><span className={`text-sm font-semibold ${entry.rank <= 3 ? "text-primary" : "text-muted-foreground"}`}>{entry.rank <= 3 ? `#${entry.rank}` : entry.rank}</span><div className="min-w-0 flex-1"><div className="font-semibold">{entry.display_name ?? entry.username} {flag(entry.country_code)}</div><div className="text-xs text-muted-foreground">@{entry.username} · {entry.projects} {entry.projects === 1 ? "project" : "projects"}</div></div><div className="text-sm font-semibold">{formatDuration(entry.credited_seconds)}</div></a></CardContent></Card>)}</div>}
    </section>
  )
}
