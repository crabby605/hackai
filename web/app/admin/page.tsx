import { notFound } from "next/navigation"
import { getJson } from "@/app/lib/api"
import AccountsTable, { type Account } from "./accounts-table"
import StorageCheck from "./storage-check"
import { Badge } from "@/components/ui/badge"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"

type Me = { is_admin: boolean }
type AdminInsights = {
  users: number; public_profiles: number; projects: number; episodes: number; credited_seconds: number; active_installations: number
  active_users_7d: number; resolution_rate_permille: number; failed_batches: number
  harnesses: { harness: string; sessions: number }[]
  projects_list: { id: string; name: string; owner_id: string; username: string; display_name: string | null; episodes: number; credited_seconds: number; last_active: number | null }[]
}

function duration(seconds: number) {
  const minutes = Math.floor(seconds / 60)
  return minutes >= 60 ? `${Math.floor(minutes / 60)}h ${minutes % 60}m` : `${minutes}m`
}

export default async function AdminPage() {
  const me = await getJson<Me>("/api/v1/me")
  if (!me?.is_admin) notFound()
  const [accounts, insights] = await Promise.all([
    getJson<Account[]>("/api/v1/admin/users"),
    getJson<AdminInsights>("/api/v1/admin/insights"),
  ])
  const metrics = [
    ["Accounts", insights?.users ?? 0, `${insights?.public_profiles ?? 0} public profiles`],
    ["Active this week", insights?.active_users_7d ?? 0, "Users with a tracked session"],
    ["Projects", insights?.projects ?? 0, "Across all accounts"],
    ["Sessions", insights?.episodes ?? 0, `${duration(insights?.credited_seconds ?? 0)} credited`],
    ["Resolution rate", `${((insights?.resolution_rate_permille ?? 0) / 10).toFixed(1)}%`, "Recent sessions"],
    ["Active installs", insights?.active_installations ?? 0, "CLI installations"],
    ["Failed batches", insights?.failed_batches ?? 0, "Needs review if non-zero"],
  ]
  return <section className="dashboard">
    <div className="dashboard-heading"><div><p className="eyebrow">Admin</p><h1>Workspace<br /><em>health.</em></h1><p className="heading-copy">Operational metrics from the data HackAI already records.</p></div></div>
    <div className="metrics-strip">{metrics.map(([label, value, note]) => <div key={String(label)}><span>{label}</span><strong>{value}</strong><small>{note}</small></div>)}</div>

    <section className="mt-10">
      <div className="section-heading"><div><h2>Most used harnesses</h2><p>Derived from recorded sessions</p></div></div>
      <div className="mt-3 flex flex-wrap gap-2">{insights?.harnesses.length ? insights.harnesses.map((item) => <Badge key={item.harness} variant="secondary">{item.harness} · {item.sessions}</Badge>) : <span className="text-sm text-muted-foreground">No harnesses recorded yet.</span>}</div>
    </section>

    <section className="mt-10">
      <div className="section-heading"><div><h2>Projects</h2><p>Most recently active projects across every account</p></div></div>
      <div className="review-table-wrap mt-3"><Table className="review-table"><TableHeader><TableRow><TableHead>Project</TableHead><TableHead>Owner</TableHead><TableHead>Sessions</TableHead><TableHead>Credited</TableHead><TableHead>Last active</TableHead></TableRow></TableHeader><TableBody>{insights?.projects_list.length ? insights.projects_list.map((project) => <TableRow key={project.id}><TableCell className="font-medium">{project.name}</TableCell><TableCell><a className="underline underline-offset-4" href={`/admin/users/${encodeURIComponent(project.owner_id)}`}>{project.display_name ?? project.username}</a></TableCell><TableCell>{project.episodes}</TableCell><TableCell>{duration(project.credited_seconds)}</TableCell><TableCell>{project.last_active ? new Date(project.last_active).toLocaleString() : "—"}</TableCell></TableRow>) : <TableRow><TableCell colSpan={5} className="admin-empty">No projects recorded.</TableCell></TableRow>}</TableBody></Table></div>
    </section>

    <section className="mt-10"><div className="section-heading"><div><h2>Accounts</h2><p>Search an account to inspect its projects and sessions.</p></div></div><div className="mt-3"><AccountsTable accounts={accounts ?? []} /></div></section>
    <StorageCheck />
  </section>
}
