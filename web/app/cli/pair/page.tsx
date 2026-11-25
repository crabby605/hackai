"use client"

import { useState } from "react"
import { useSearchParams } from "next/navigation"
import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export default function CliPairPage() {
  const params = useSearchParams()
  const code = params.get("code")
  const [status, setStatus] = useState<"confirm" | "approving" | "approved" | "error">("confirm")

  async function approve() {
    if (!code) { setStatus("error"); return }
    setStatus("approving")
    try {
      const response = await fetch("/api/v1/cli/pairings/complete", {
        method: "POST",
        headers: { "content-type": "application/json" },
        credentials: "same-origin",
        body: JSON.stringify({ code }),
      })
      if (response.status === 401) {
        const continuePath = `${window.location.pathname}${window.location.search}`
        window.location.assign(`/login?and=${encodeURIComponent(continuePath)}`)
        return
      }
      setStatus(response.ok ? "approved" : "error")
    } catch {
      setStatus("error")
    }
  }

  return <section className="state-card">
    <p className="eyebrow">HackAI CLI</p>
    {status === "confirm" && <>
      <h1>Approve this<br /><em>device?</em></h1>
      <p>Only continue if you started <code>hackai login</code> on this device. Approving links its installation token to your account.</p>
      {code ? <p>Device code: <code>{code}</code></p> : <p>Missing device code. Run <code>hackai login</code> again.</p>}
      <button className={cn(buttonVariants({ size: "lg" }))} disabled={!code} onClick={approve}>Approve device</button>
    </>}
    {status === "approving" && <><h1>Approving this<br /><em>device.</em></h1><p>One moment—your local CLI is being linked to this account.</p></>}
    {status === "approved" && <><h1>Device approved.</h1><p>Return to your terminal. HackAI can now sync using this installation’s token.</p></>}
    {status === "error" && <><h1>Couldn’t approve<br /><em>this device.</em></h1><p>This sign-in link may have expired or already been used. Return to the terminal and run <code>hackai login</code> again.</p></>}
  </section>
}
