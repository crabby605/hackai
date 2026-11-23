import { getJson } from "@/app/lib/api"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"
import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

interface Profile {
  username: string
  display_name: string | null
  country_code: string | null
  is_admin: boolean
  avatar_url: string | null
}

function flag(country: string | null) {
  return country && /^[A-Z]{2}$/.test(country)
    ? String.fromCodePoint(...[...country].map((letter) => 127397 + letter.charCodeAt(0)))
    : ""
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&")
    .replace(/</g, "<")
    .replace(/>/g, ">")
    .replace(/"/g, '"')
    .replace(/'/g, "&#039;")
    .replace(/[\x00-\x1F\x7F]/g, (c) => `&#x${c.charCodeAt(0).toString(16).padStart(4, "0")};`);
}

export default async function ProfilePage({
  params,
}: {
  params: Promise<{ username: string }>
}) {
  const { username } = await params
  const profile = (await getJson<Profile>(
    `/api/v1/users/${encodeURIComponent(username)}`
  )) ?? null

  if (!profile) {
    return (
      <section className="state-card">
        <p className="eyebrow">Profile</p>
        <h1 style={{ fontSize: "1.75rem", letterSpacing: "-0.04em", margin: "0 0 1rem" }}>
          No profile for <em>@{escapeHtml(username)}</em>
        </h1>
        <p>This user hasn’t registered or hasn’t shared any public activity yet.</p>
        <a className={cn(buttonVariants({ variant: "outline" }))} href="/">
          Back to home <span>→</span>
        </a>
      </section>
    )
  }

  return (
    <section className="state-card">
      <p className="eyebrow">Public builder profile</p>
      <h1 style={{ fontSize: "1.75rem", letterSpacing: "-0.04em", margin: "0 0 0.25rem" }}>
        {profile.display_name ?? `@${profile.username}`} {flag(profile.country_code)}
      </h1>
      {profile.display_name && (
        <p className="text-sm text-muted-foreground" style={{ margin: "0 0 0.75rem" }}>
          @{profile.username}
        </p>
      )}
      <Avatar className="profile-avatar" aria-label={`${profile.username}'s profile image`} style={{ marginBottom: "1rem" }}><AvatarFallback>{(profile.display_name ?? profile.username).slice(0, 2).toUpperCase()}</AvatarFallback>{profile.avatar_url && <AvatarImage src={profile.avatar_url} alt="" referrerPolicy="no-referrer" />}</Avatar>
      {profile.is_admin && (
        <p className="mb-4 text-sm font-semibold text-muted-foreground">
          Administrator
        </p>
      )}
      <p>Verified HackAI builder. Activity details remain private.</p>
    </section>
  )
}
