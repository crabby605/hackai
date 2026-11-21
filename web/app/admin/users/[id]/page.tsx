import { notFound } from "next/navigation"
import { getJson } from "@/app/lib/api"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"
import { buttonVariants } from "@/components/ui/button"
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/ui/accordion"
import { Card, CardContent } from "@/components/ui/card"
import { cn } from "@/lib/utils"

type Me = { is_admin: boolean }
type Project = { id: string; name: string; remote_url: string | null; episodes: number; credited_seconds: number }
type Episode = {
  id: string; batch_id: string; project_id: string; session_key: string; harness: string | null; started_at: number; ended_at: number
  attempts: number; resolved: number; credited_seconds: number; human_edit_seconds: number; agent_edit_seconds: number
  signatures_json: string; files_touched_json: string; evidence_ids_json: string; formula_version: number
  batch_status: string; batch_bytes: number; batch_received_at: number
}
type Account = {
  username: string; display_name: string | null; avatar_url: string | null; country_code: string | null; profile_public: boolean
  projects: Project[]; episodes: Episode[]; installations: { id: string }[]
}

const languageByExtension: Record<string, string> = {
  ts: "TypeScript", tsx: "TypeScript (React)", js: "JavaScript", jsx: "JavaScript (React)", mjs: "JavaScript", cjs: "JavaScript",
  py: "Python", go: "Go", rs: "Rust", java: "Java", kt: "Kotlin", kts: "Kotlin", swift: "Swift", rb: "Ruby", php: "PHP",
  cs: "C#", c: "C", h: "C/C++ header", cc: "C++", cpp: "C++", hpp: "C++ header", sql: "SQL", html: "HTML",
  css: "CSS", scss: "SCSS", sass: "Sass", vue: "Vue", svelte: "Svelte", json: "JSON", yml: "YAML", yaml: "YAML",
  md: "Markdown", sh: "Shell", bash: "Shell", zsh: "Shell", toml: "TOML", xml: "XML", dockerfile: "Dockerfile",
}

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

function episodeLanguages(files: string[]) {
  const found = new Set<string>()
  for (const file of files) {
    const name = file.split("/").at(-1)?.toLowerCase() ?? ""
    if (name === "dockerfile") found.add("Dockerfile")
    const extension = name.split(".").at(-1) ?? ""
    if (languageByExtension[extension]) found.add(languageByExtension[extension])
  }
  return [...found]
}

function struggleSignal(episode: Episode) {
  if (!episode.resolved && episode.attempts >= 4) return "High — unresolved after repeated attempts"
  if (!episode.resolved) return "High — unresolved"
  if (episode.attempts >= 4) return "Elevated — repeated attempts before resolution"
  if (episode.attempts >= 2) return "Moderate — more than one attempt"
  return "Low — resolved on first attempt"
}

function Cell({ label, children }: { label: string; children: React.ReactNode }) {
  return <div><div className="item-meta">{label}</div><div style={{ overflowWrap: "anywhere" }}>{children}</div></div>
}

export default async function AccountReview({ params }: { params: Promise<{ id: string }> }) {
  const me = await getJson<Me>("/api/v1/me")
  if (!me?.is_admin) notFound()
  const { id } = await params
  const account = await getJson<Account>(`/api/v1/admin/users/${encodeURIComponent(id)}`)
  if (!account) notFound()

  return <section className="dashboard">
    <a className={cn(buttonVariants({ variant: "outline" }))} href="/admin">← All accounts</a>
    <div className="dashboard-heading" style={{ marginTop: "1.5rem" }}><div><p className="eyebrow">Account review</p><h1>{account.display_name ?? account.username}<br /><em>@{account.username}</em></h1><p className="heading-copy">{account.profile_public ? "Public" : "Private"} · {account.country_code ?? "—"} · {account.installations.length} installation(s)</p></div><Avatar className="profile-avatar" aria-label={`${account.username}'s profile image`}><AvatarFallback>{(account.display_name ?? account.username).slice(0, 2).toUpperCase()}</AvatarFallback>{account.avatar_url && <AvatarImage src={account.avatar_url} alt="" referrerPolicy="no-referrer" />}</Avatar></div>

    <h2>Projects</h2>
    <div className="space-y-2">{account.projects.length ? account.projects.map((project) => <Card key={project.id} size="sm"><CardContent className="flex items-center justify-between gap-4"><div><div className="font-semibold">{project.name}</div><div className="text-xs text-muted-foreground">{project.remote_url ?? "No remote URL"}</div></div><div className="text-sm font-medium">{project.episodes} episodes · {Math.round(project.credited_seconds / 60)} min</div></CardContent></Card>) : <p className="text-sm text-muted-foreground">No projects recorded.</p>}</div>

    <h2 style={{ marginTop: "2rem" }}>Session and struggle review</h2>
    <p className="heading-copy">Difficulty and language labels are derived only from captured episode metadata and filenames.</p>
    {account.episodes.length ? <Accordion className="rounded-lg border" defaultValue={[]} multiple>{account.episodes.map((episode) => {
      const files = jsonList(episode.files_touched_json)
      const evidence = jsonList(episode.evidence_ids_json)
      const signatures = jsonList(episode.signatures_json)
      const languages = episodeLanguages(files)
      return <AccordionItem value={episode.id} key={episode.id}>
        <AccordionTrigger className="px-4 hover:no-underline">
          <span><strong>{episode.session_key}</strong><br /><span className="item-meta">{new Date(episode.started_at).toLocaleString()} · {duration(episode.ended_at - episode.started_at)} · {episode.resolved ? "Resolved" : "Unresolved"}</span></span>
          <span className="item-value">{struggleSignal(episode)}</span>
        </AccordionTrigger>
        <AccordionContent className="border-t px-4 pt-4">
        <div style={{ display: "grid", gap: "1rem" }}>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(170px, 1fr))", gap: "1rem" }}>
            <Cell label="Session duration">{duration(episode.ended_at - episode.started_at)}</Cell>
            <Cell label="Struggle signal (derived)">{struggleSignal(episode)}</Cell>
            <Cell label="Attempts / outcome">{episode.attempts} / {episode.resolved ? "resolved" : "unresolved"}</Cell>
            <Cell label="Human / agent work">{duration(episode.human_edit_seconds * 1000)} / {duration(episode.agent_edit_seconds * 1000)}</Cell>
            <Cell label="Credited time">{duration(episode.credited_seconds * 1000)}</Cell>
            <Cell label="Detected languages">{languages.length ? languages.join(", ") : "No recognized file extension"}</Cell>
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(230px, 1fr))", gap: "1rem" }}>
            <Cell label="Started / ended">{new Date(episode.started_at).toISOString()}<br />{new Date(episode.ended_at).toISOString()}</Cell>
            <Cell label="Harness">{episode.harness ?? "Not recorded"}</Cell>
            <Cell label="Session key"><code>{episode.session_key}</code></Cell>
            <Cell label="Project ID"><code>{episode.project_id}</code></Cell>
            <Cell label="Batch / ingestion"><code>{episode.batch_id}</code><br />{episode.batch_status} · {episode.batch_bytes.toLocaleString()} bytes · received {new Date(episode.batch_received_at).toISOString()}</Cell>
            <Cell label="Formula version">v{episode.formula_version}</Cell>
          </div>
          <Cell label={`Files touched (${files.length})`}>{files.length ? <ul style={{ margin: "0.4rem 0 0", paddingLeft: "1.1rem" }}>{files.map((file) => <li key={file}><code>{file}</code></li>)}</ul> : "No files were recorded."}</Cell>
          <Cell label={`Evidence IDs (${evidence.length})`}>{evidence.length ? <ul style={{ margin: "0.4rem 0 0", paddingLeft: "1.1rem" }}>{evidence.map((item) => <li key={item}><code>{item}</code></li>)}</ul> : "No evidence IDs were recorded."}</Cell>
          <Cell label={`Change signatures (${signatures.length})`}>{signatures.length ? <ul style={{ margin: "0.4rem 0 0", paddingLeft: "1.1rem" }}>{signatures.map((item) => <li key={item}><code>{item}</code></li>)}</ul> : "No signatures were recorded."}</Cell>
        </div>
        </AccordionContent>
      </AccordionItem>
    })}</Accordion> : <Card><CardContent><p className="text-sm text-muted-foreground">No episodes recorded.</p></CardContent></Card>}
  </section>
}
