import "server-only"
import { cookies, headers } from "next/headers"

// This module is server-only. Keep server rendering on the local/private API
// route instead of sending authenticated requests back through the public edge.
const API_BASE = process.env.INTERNAL_API_URL || "http://127.0.0.1:3000"

// HTTPS sets __Host-hackai_session (Secure); local HTTP sets hackai_session
// (non-Secure). Read/send whichever cookie the browser actually holds.
const SESSION_COOKIE_HOST = "__Host-hackai_session"
const SESSION_COOKIE_PLAIN = "hackai_session"

type SessionCookie = { name: string; value: string }

type SessionUser = {
  id: string
  username: string
  display_name: string | null
  avatar_url: string | null
  country_code: string | null
  profile_public: boolean
  leaderboard_opt_out: boolean
  is_admin: boolean
}

const weekdayNames = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]
const languageByExtension: Record<string, string> = {
  ts: "TypeScript", tsx: "TypeScript", js: "JavaScript", jsx: "JavaScript",
  rs: "Rust", py: "Python", go: "Go", java: "Java", kt: "Kotlin", swift: "Swift",
  rb: "Ruby", php: "PHP", cs: "C#", cpp: "C++", c: "C", h: "C/C++",
  html: "HTML", css: "CSS", sql: "SQL", sh: "Shell", bash: "Shell", zsh: "Shell",
  json: "JSON", yaml: "YAML", yml: "YAML", md: "Markdown"
}

function extensionLanguage(path: string) {
  return languageByExtension[path.split(".").pop()?.toLowerCase() ?? ""] ?? null
}

async function getCookieSession(): Promise<SessionCookie | undefined> {
  const cookieStore = await cookies()
  for (const name of [SESSION_COOKIE_HOST, SESSION_COOKIE_PLAIN]) {
    const value = cookieStore.get(name)?.value
    if (value) return { name, value }
  }
  return undefined
}

async function fetchApi<T>(path: string): Promise<T | null> {
  const session = await getCookieSession()

  const reqHeaders: Record<string, string> = { "Content-Type": "application/json" }
  if (session) reqHeaders.Cookie = `${session.name}=${session.value}`
  try {
    const headerStore = await headers()
    const forwarded = headerStore.get("x-forwarded-for")
    if (forwarded) {
      reqHeaders["x-forwarded-for"] = forwarded
    }
    const realIp = headerStore.get("x-real-ip")
    if (realIp) {
      reqHeaders["x-real-ip"] = realIp
    }
  } catch {
    // headers() may throw in non-request contexts
  }
  try {
    const res = await fetch(`${API_BASE}${path}`, {
      headers: reqHeaders,
      cache: "no-store",
    })
    if (!res.ok) return null
    return res.json() as Promise<T>
  } catch {
    // A local UI preview may deliberately run without the Rust API. Pages use
    // null to render their existing unavailable/empty state rather than crash.
    return null
  }
}

export async function getJson<T>(path: string): Promise<T | null> {
  return fetchApi<T>(path)
}
