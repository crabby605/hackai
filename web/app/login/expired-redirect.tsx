"use client"

import { useEffect, useState } from "react"

export default function ExpiredRedirect({ continueUrl }: { continueUrl: string }) {
  const [retried, setRetried] = useState(false)

  useEffect(() => {
    const key = `auth_retry_${continueUrl}`
    const alreadyRetried = sessionStorage.getItem(key)
    if (!alreadyRetried) {
      sessionStorage.setItem(key, "1")
      const timer = window.setTimeout(() => {
        window.location.assign(`/auth/login?continue=${continueUrl}`)
      }, 900)
      return () => window.clearTimeout(timer)
    } else {
      setRetried(true)
    }
  }, [continueUrl])

  return (
    <p role="status" className="login-expired">
      {retried
        ? "Your session has expired. Please click Continue to sign in again."
        : "Your login expired, redirecting you to sign in…"}
    </p>
  )
}
