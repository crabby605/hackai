import "server-only"
import { cookies } from "next/headers"

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:3000"

type SessionUser = {
  id: string
  username: string
  display_name: string | null
  avatar_url: string | null
  country_code: string | null
  profile_public: number
  leaderboard_opt_out: number
  is_admin: number
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

async function getCookieSession(): Promise<string | undefined> {
  const cookieStore = await cookies()
  return cookieStore.get("hackai_session")?.value
}

async function fetchApi<T>(path: string): Promise<T | null> {
  const session = await getCookieSession()
  if (!session) return null

  const res = await fetch(`${API_BASE}${path}`, {
    headers: {
      "Cookie": `hackai_session=${session}`,
      "Content-Type": "application/json",
    },
    cache: "no-store",
  })

  if (!res.ok) return null
  return res.json() as Promise<T>
}

export async function getJson<T>(path: string): Promise<T | null> {
  return fetchApi<T>(path)
}