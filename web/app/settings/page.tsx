import { redirect } from "next/navigation"
import { getJson } from "@/app/lib/api"
import ProfileBuilder from "./profile-builder"
import ApiKeyCard from "./api-key-card"

interface Me { username: string; display_name: string | null; avatar_url: string | null; profile_public: boolean; leaderboard_opt_out: boolean; country_code: string | null }

export default async function SettingsPage() {
  const me = await getJson<Me>("/api/v1/me")
  if (!me) redirect("/login")

  return <><ProfileBuilder initialProfile={me} /><section className="settings-page settings-api-key"><ApiKeyCard /></section></>
}
