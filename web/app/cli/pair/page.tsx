"use client"

import { useEffect, useState } from "react"
import { useSearchParams } from "next/navigation"

export default function CliPairPage() {
  const params = useSearchParams()
  const code = params.get("code")
  const [status, setStatus] = useState<"approving" | "approved" | "error">("approving")

  useEffect(() => {
    if (!code) { setStatus("error"); return }
    void fetch("/api/v1/cli/pairings/complete", {
      method: "POST",
      headers: { "content-type": "application/json" },
      credentials: "same-origin",
      body: JSON.stringify({ code }),
    }).then((response) => setStatus(response.ok ? "approved" : "error")).catch(() => setStatus("error"))
  }, [code])

  return <section className="state-card">
    <p className="eyebrow">HackAI CLI</p>
    {status === "approving" && <><h1>Approving this<br /><em>device.</em></h1><p>One moment—your local CLI is being linked to this account.</p></>}
    {status === "approved" && <><h1>Device approved.</h1><p>Return to your terminal. HackAI can now sync using this installation’s token.</p></>}
    {status === "error" && <><h1>Couldn’t approve<br /><em>this device.</em></h1><p>This sign-in link may have expired or already been used. Return to the terminal and run <code>hackai login</code> again.</p></>}
  </section>
}
