import { cookies } from "next/headers"
import { redirect } from "next/navigation"
import { Button } from "@/components/ui/button"
import { getJson } from "@/app/lib/api"
import ExpiredRedirect from "./expired-redirect"

function localPath(value: string | undefined): string {
  if (!value || !value.startsWith("/") || value.includes("\\") || /%2f|%5c/i.test(value)) return "/"
  try {
    const target = new URL(value, "http://localhost:3001")
    return target.origin === "http://localhost:3001" ? `${target.pathname}${target.search}${target.hash}` : "/"
  } catch { return "/" }
}

export default async function LoginPage({
  searchParams,
}: {
    searchParams: Promise<{ and?: string; error?: string; expired?: string; utm_source?: string; utm_medium?: string }>
}) {
  const { and, error, expired, utm_source, utm_medium } = await searchParams
  const targetPath = localPath(and)
  const continuePath = targetPath === "/login" ? "/dashboard" : targetPath
  const cookieStore = await cookies()
  const cookie = cookieStore.get("hackai_session")?.value
  const viewer = cookie ? await getJson<{ username: string }>("/api/v1/me") : null
  const sessionExpired = expired === "1" || Boolean(cookie && !viewer)

  // A stale cookie must not bounce the user back to the page that sent them
  // here. Only a verified session is already signed in.
  if (cookie && viewer) {
    redirect(continuePath)
  }

  const continueUrl = encodeURIComponent(continuePath)

  return (
    <section className="state-card">
      <p className="eyebrow">Welcome back</p>
      <h1 style={{ fontSize: "1.75rem", letterSpacing: "-0.04em", margin: "0 0 1rem" }}>
        Sign in with Hack Club
      </h1>
      <Button
        render={<a href={`/auth/login?continue=${continueUrl}`} />}
        size="lg"
      >
        Continue <span>→</span>
      </Button>
      {sessionExpired && !error && <ExpiredRedirect continueUrl={continueUrl} />}
      {error && <p style={{ color: "var(--danger)", fontSize: "0.8rem", marginTop: "1rem" }}>
        Sign-in did not complete. Reference: <code>{error}</code>{utm_source && utm_medium ? ` (${utm_source}/${utm_medium})` : ""}.
      </p>}
      <p className="mt-6 text-sm leading-6 text-muted-foreground">
        HackAI only asks Hack Club Auth to identify your account. Your prompts
        and tool results are never uploaded.
      </p>
    </section>
  )
}
