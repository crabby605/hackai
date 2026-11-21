"use client"

import { useEffect, useState } from "react"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select"

type Project = { id: string; name: string }
type Manifest = { id: string; project_id: string; project_name: string; object_key: string; bytes: number; status: string; received_at: number; archived_at: number | null }

export default function ArchiveBrowser({ admin = false, initialOwnerId = "" }: { admin?: boolean; initialOwnerId?: string }) {
  const [ownerId, setOwnerId] = useState(initialOwnerId)
  const [projects, setProjects] = useState<Project[]>([])
  const [projectId, setProjectId] = useState("")
  const [from, setFrom] = useState("")
  const [to, setTo] = useState("")
  const [items, setItems] = useState<Manifest[] | null>(null)
  const [message, setMessage] = useState("")

  useEffect(() => {
    if (admin) { setProjects([]); return }
    void fetch("/api/v1/projects", { credentials: "include" }).then(async (response) => {
      if (!response.ok) return
      setProjects((await response.json()) as Project[])
    })
  }, [admin, ownerId])

  async function fetchArchives() {
    if (admin && !ownerId.trim()) { setMessage("Enter an account ID from the account review URL first."); return }
    setMessage("")
    const params = new URLSearchParams()
    if (admin) params.set("user_id", ownerId.trim())
    if (projectId) params.set("project_id", projectId)
    if (from) params.set("from", from)
    if (to) params.set("to", to)
    const response = await fetch(`${admin ? "/api/v1/admin/archives" : "/api/v1/archives"}?${params}`, { credentials: "include" })
    if (!response.ok) { setMessage("Couldn’t load archive manifests."); return }
    const body = await response.json() as { manifests: Manifest[]; projects: Project[] }
    setItems(body.manifests)
    setProjects(body.projects)
  }

  return <Card className="mt-10" size="sm">
    <CardHeader><div><h2 className="text-base">Archive lookup</h2><p className="text-sm text-muted-foreground">Find up to 100 archived sync batches in R2 by project and received date. The raw payload stays protected.</p></div></CardHeader>
    <CardContent className="space-y-4">
      <div className="grid gap-3 md:grid-cols-4">
        {admin && <Input value={ownerId} onChange={(event) => { setOwnerId(event.target.value); setProjectId("") }} placeholder="Account UUID" aria-label="Account UUID" />}
        <NativeSelect value={projectId} onChange={(event) => setProjectId(event.target.value)} className="w-full"><NativeSelectOption value="">All projects</NativeSelectOption>{projects.map((project) => <NativeSelectOption key={project.id} value={project.id}>{project.name}</NativeSelectOption>)}</NativeSelect>
        <Input type="date" value={from} onChange={(event) => setFrom(event.target.value)} aria-label="Received from date" />
        <Input type="date" value={to} onChange={(event) => setTo(event.target.value)} aria-label="Received through date" />
        <Button onClick={fetchArchives}>{items === null ? "Find archives" : "Refresh"}</Button>
      </div>
      {message && <p className="text-sm text-destructive">{message}</p>}
      {items && (items.length ? <div className="space-y-2">{items.map((item) => <div key={item.id} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border px-3 py-2 text-sm"><div><strong>{item.project_name}</strong><span className="ml-2 text-muted-foreground">{new Date(item.received_at).toLocaleString()} · {item.bytes.toLocaleString()} B · {item.status}</span></div><code className="max-w-full truncate text-xs text-muted-foreground" title={item.object_key}>{item.archived_at ? "R2 · " : "Awaiting archive · "}{item.object_key}</code></div>)}</div> : <p className="text-sm text-muted-foreground">No archive manifests match these filters.</p>)}
    </CardContent>
  </Card>
}
