"use client"

import { useEffect } from "react"

export default function ExpiredRedirect({ continueUrl }: { continueUrl: string }) {
  useEffect(() => {
    const timer = window.setTimeout(() => {
      window.location.assign(`/auth/login?continue=${continueUrl}`)
    }, 900)
    return () => window.clearTimeout(timer)
  }, [continueUrl])

  return <p role="status" className="login-expired">Your login expired, logging you in again…</p>
}
