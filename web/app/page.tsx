import { cookies } from "next/headers"
import DashboardPage from "./dashboard/page"
import { getJson } from "@/app/lib/api"
import { MarketingHome } from "@/components/marketing-home"

export default async function Home() {
  const cookieStore = await cookies()
  const cookie = cookieStore.get("__Host-hackai_session")?.value ?? cookieStore.get("hackai_session")?.value
  if (cookie && await getJson("/api/v1/me")) return <DashboardPage />
  return <MarketingHome />
}
