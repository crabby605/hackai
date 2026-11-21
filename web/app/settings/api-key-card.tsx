"use client"

import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader } from "@/components/ui/card"

export default function ApiKeyCard() {
  const [key, setKey] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState("")

  async function rotate() {
    setBusy(true)
    setMessage("")
    try {
      const response = await fetch("/api/v1/me/api-key", { method: "POST", credentials: "include" })
      if (!response.ok) throw new Error("Couldn’t rotate the API key.")
      const body = await response.json() as { api_key?: string }
      if (!body.api_key) throw new Error("The server returned no API key.")
      setKey(body.api_key)
      setMessage("New key created. Copy it now; it won’t be shown again.")
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Couldn’t rotate the API key.")
    } finally {
      setBusy(false)
    }
  }

  async function copy() {
    if (!key) return
    await navigator.clipboard.writeText(key)
    setMessage("Copied. Keep it private.")
  }

  return (
    <Card className="settings-card">
      <CardHeader className="settings-card-heading"><div><h2>CLI API key</h2><p>Use this key with <code>hackai login --api-key</code> or <code>HACKAI_API_KEY</code>. Only one active key is kept.</p></div></CardHeader>
      <CardContent>
        <div className="api-key-row">
          <code aria-label="API key, hidden">{key ? key.slice(-10) : "••••••••••"}</code>
          {key && <Button type="button" variant="outline" onClick={copy}>Copy</Button>}
          <Button type="button" onClick={rotate} disabled={busy}>{busy ? "Rotating…" : "Rotate key"}</Button>
        </div>
        <p className="field-help" role="status">{message || "The full key is never displayed and cannot be recovered after this page is closed."}</p>
      </CardContent>
    </Card>
  )
}
