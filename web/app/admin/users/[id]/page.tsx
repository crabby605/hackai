import { notFound } from "next/navigation"
import { getJson } from "@/app/lib/api"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"
import { buttonVariants } from "@/components/ui/button"
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/ui/accordion"
import { Card, CardContent } from "@/components/ui/card"
import { cn } from "@/lib/utils"

type Me = { is_admin: boolean }
type Project = { id: string; name: string; episodes: number; credited_seconds: number }
type Episode = {
  id: string; batch_id: string; project_id: string; harness: string | null; started_at: number; ended_at: number
  attempts: number; resolved: number; credited_seconds: number; human_edit_seconds: number; agent_edit_seconds: number
  evidence_ids_json: string; formula_version: number
  batch_status: string; batch_bytes: number; batch_received_at: number
}
type Account = {
  username: string; display_name: string | null; avatar_url: string | null; country_code: string | null; profile_public: boolean
  projects: Project[]; episodes: Episode[]; installations: { id: string }[]
}
type EvidenceBundle = { bundle_id: string; processor_version: string; formula_version: string; received_at: string; event_count: number; preview_approved: boolean }

function jsonList(value: string) {
  try {
    const parsed: unknown = JSON.parse(value)
    return Array.isArray(parsed) ? parsed.filter((item): item is string => typeof item === "string") : []
  } catch { return [] }
}

function duration(milliseconds: number) {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000))
  const hours = Math.floor(seconds / 3600); const minutes = Math.floor((seconds % 3600) / 60); const rest = seconds % 60
  return hours ? `${hours}h ${minutes}m ${rest}s` : minutes ? `${minutes}m ${rest}s` : `${rest}s`
}

function Cell({ label, children }: { label: string; children: React.ReactNode }) {
  return <div><div className="item-meta">{label}</div><div style={{ overflowWrap: "anywhere" }}>{children}</div></div>
}

export default async function AccountReview({ params }: { params: Promise<{ id: string }> }) {
  const me = await getJson<Me>("/api/v1/me")
  if (!me?.is_admin) notFound()
  const { id } = await params
  const [account, bundles] = await Promise.all([
    getJson<Account>(`/api/v1/admin/users/${encodeURIComponent(id)}`),
    getJson<EvidenceBundle[]>(`/api/v1/admin/users/${encodeURIComponent(id)}/evidence-bundles`),
  ])
  if (!account) notFound()

  return <section className="dashboard">
    <a className={cn(buttonVariants({ variant: "outline" }))} href="/admin">← All accounts</a>
    <div className="dashboard-heading" style={{ marginTop: "1.5rem" }}><div><p className="eyebrow">Account review</p><h1>{account.display_name ?? account.username}<br /><em>@{account.username}</em></h1><p className="heading-copy">{account.profile_public ? "Public" : "Private"} · {account.country_code ?? "—"} · {account.installations.length} installation(s)</p></div><Avatar className="profile-avatar" aria-label={`${account.username}'s profile image`}><AvatarFallback>{(account.display_name ?? account.username).slice(0, 2).toUpperCase()}</AvatarFallback>{account.avatar_url && <AvatarImage src={account.avatar_url} alt="" referrerPolicy="no-referrer" />}</Avatar></div>

    <h2>Projects</h2>
    <div className="space-y-2">{account.projects.length ? account.projects.map((project) => <Card key={project.id} size="sm"><CardContent className="flex items-center justify-between gap-4"><div><div className="font-semibold">{project.name}</div><div className="text-xs text-muted-foreground">Derived activity summary</div></div><div className="text-sm font-medium">{project.episodes} episodes · {Math.round(project.credited_seconds / 60)} min</div></CardContent></Card>) : <p className="text-sm text-muted-foreground">No projects recorded.</p>}</div>

    <h2 style={{ marginTop: "2rem" }}>Opt-in evidence exports</h2>
    <p className="heading-copy">Provenance summaries only. Reviewers do not see prompt text, code, file paths, or tool output.</p>
    {bundles?.length ? <div className="space-y-2">{bundles.map((bundle) => <Card key={bundle.bundle_id} size="sm"><CardContent className="flex flex-wrap items-center justify-between gap-3"><div><div className="font-semibold">{bundle.event_count} derived event{bundle.event_count === 1 ? "" : "s"}</div><div className="text-xs text-muted-foreground">Received {new Date(bundle.received_at).toLocaleString()} · processor {bundle.processor_version} · formula {bundle.formula_version}</div></div><span className="text-sm font-medium">{bundle.preview_approved ? "Approved export" : "Recorded export"}</span></CardContent></Card>)}</div> : <p className="text-sm text-muted-foreground">No opt-in evidence exports recorded.</p>}

    <h2 style={{ marginTop: "2rem" }}>Recorded episode review</h2>
    <p className="heading-copy">This view contains derived timing, outcome, and ingestion metadata. It does not expose prompts, tool output, file paths, or change content.</p>
    {account.episodes.length ? <Accordion className="rounded-lg border" defaultValue={[]} multiple>{account.episodes.map((episode) => {
      const evidence = jsonList(episode.evidence_ids_json)
      return <AccordionItem value={episode.id} key={episode.id}>
        <AccordionTrigger className="px-4 hover:no-underline">
          <span><strong>Episode {episode.id}</strong><br /><span className="item-meta">{new Date(episode.started_at).toLocaleString()} · {duration(episode.ended_at - episode.started_at)} · {episode.resolved ? "Resolved" : "Unresolved"}</span></span>
          <span className="item-value">{episode.attempts} pass{episode.attempts === 1 ? "" : "es"}</span>
        </AccordionTrigger>
        <AccordionContent className="border-t px-4 pt-4">
        <div style={{ display: "grid", gap: "1rem" }}>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(170px, 1fr))", gap: "1rem" }}>
            <Cell label="Session duration">{duration(episode.ended_at - episode.started_at)}</Cell>
            <Cell label="Attempts / outcome">{episode.attempts} / {episode.resolved ? "resolved" : "unresolved"}</Cell>
            <Cell label="Human / agent work">{duration(episode.human_edit_seconds * 1000)} / {duration(episode.agent_edit_seconds * 1000)}</Cell>
            <Cell label="Credited time">{duration(episode.credited_seconds * 1000)}</Cell>
            <Cell label="Derived evidence records">{evidence.length}</Cell>
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(230px, 1fr))", gap: "1rem" }}>
            <Cell label="Started / ended">{new Date(episode.started_at).toISOString()}<br />{new Date(episode.ended_at).toISOString()}</Cell>
            <Cell label="Harness">{episode.harness ?? "Not recorded"}</Cell>
            <Cell label="Project ID"><code>{episode.project_id}</code></Cell>
            <Cell label="Batch / ingestion"><code>{episode.batch_id}</code><br />{episode.batch_status} · {episode.batch_bytes.toLocaleString()} bytes · received {new Date(episode.batch_received_at).toISOString()}</Cell>
            <Cell label="Formula version">v{episode.formula_version}</Cell>
          </div>
          <Cell label="Evidence references">{evidence.length ? "Derived evidence is available for the episode." : "No derived evidence records were uploaded."}</Cell>
        </div>
        </AccordionContent>
      </AccordionItem>
    })}</Accordion> : <Card><CardContent><p className="text-sm text-muted-foreground">No episodes recorded.</p></CardContent></Card>}
  </section>
}
