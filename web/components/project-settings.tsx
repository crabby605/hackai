"use client"

import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet"

export function ProjectSettings({ projectId, initialName, initialGitUrl }: { projectId: string; initialName: string; initialGitUrl: string | null }) {
  const [name, setName] = useState(initialName)
  const [gitUrl, setGitUrl] = useState(initialGitUrl ?? "")
  const [status, setStatus] = useState("")
  const [saving, setSaving] = useState(false)
  async function save() {
    setSaving(true)
    setStatus("")
    try {
      const response = await fetch(`/api/v1/projects/${encodeURIComponent(projectId)}`, { method: "PATCH", credentials: "include", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ name, git_url: gitUrl }) })
      if (!response.ok) throw new Error("save failed")
      setStatus("Saved. Refreshing project…")
      window.location.reload()
    } catch { setStatus("Couldn’t save the project. Use a valid HTTP(S) Git URL and try again.") } finally { setSaving(false) }
  }
  return <Sheet><SheetTrigger render={<Button variant="outline" size="sm" />}>Edit project</SheetTrigger><SheetContent><SheetHeader><SheetTitle>Edit project</SheetTitle><SheetDescription>Rename this project or update the Git remote used for its project record.</SheetDescription></SheetHeader><div className="grid gap-4 px-4"><label className="grid gap-2 text-sm font-medium" htmlFor="project-name">Project name<Input id="project-name" value={name} onChange={(event) => setName(event.target.value)} maxLength={128} /></label><label className="grid gap-2 text-sm font-medium" htmlFor="project-git-url">Git URL<Input id="project-git-url" value={gitUrl} onChange={(event) => setGitUrl(event.target.value)} placeholder="https://github.com/owner/repository" type="url" maxLength={2048} /></label>{status && <p className="text-sm text-muted-foreground" role="status">{status}</p>}</div><SheetFooter><Button onClick={save} disabled={saving || !name.trim()}>{saving ? "Saving…" : "Save changes"}</Button></SheetFooter></SheetContent></Sheet>
}
