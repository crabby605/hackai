import "./globals.css"
import { cookies } from "next/headers"
import { getJson } from "@/app/lib/api"
import { cn } from "@/lib/utils";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { WorkspaceShell } from "@/components/workspace-shell";
import { WorkspaceNavLink } from "@/components/workspace-nav-link";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarSeparator,
} from "@/components/ui/sidebar";
import { LayoutDashboard, BarChart3, CircleDot, FileCheck2, FolderGit2, LogOut, Settings, ShieldCheck } from "lucide-react";
import type { LucideIcon } from "lucide-react";

interface Viewer { display_name: string | null; username: string; email: string | null; country_code: string | null; avatar_url: string | null; is_admin: boolean }

function flag(country: string | null | undefined) {
  return country && /^[A-Z]{2}$/.test(country) ? String.fromCodePoint(...[...country].map((letter) => 127397 + letter.charCodeAt(0))) : ""
}

const nav: Array<[string, string, LucideIcon]> = [
  ["Overview", "/dashboard", LayoutDashboard],
  ["Projects", "/projects", FolderGit2],
  ["Struggles", "/struggles", CircleDot],
  ["Evidence", "/evidence", FileCheck2],
  ["Leaderboard", "/leaderboard", BarChart3],
]

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  let viewer = await getJson<Viewer>("/api/v1/me")
  const cookieStore = await cookies()
  // Production HTTPS uses the __Host cookie; local HTTP uses this fallback.
  const cookie = cookieStore.get("__Host-hackai_session")?.value ?? cookieStore.get("hackai_session")?.value
  // The standalone local UI preview runs without the Rust API. Keep its
  // workspace shell visible so routes can be reviewed in their real chrome.
  if (process.env.NODE_ENV === "development" && !viewer) {
    viewer = { display_name: "Vihaan", username: "vihaan", email: null, country_code: "IN", avatar_url: null, is_admin: true }
  }
  const name = viewer?.display_name ?? viewer?.username

  // Public marketing and auth pages should not inherit the workspace chrome.
  // The sidebar is only meaningful once a session exists.
  // A stale/invalid cookie must not expose workspace navigation on public or
  // auth pages. Only a verified API viewer gets the sidebar.
  if ((!cookie || !viewer) && process.env.NODE_ENV !== "development") {
    return (
      <html lang="en" className={cn("dark font-sans")}>
        <body className="public-shell">{children}</body>
      </html>
    )
  }

  return (
    <html lang="en" className={cn("dark font-sans")}>
      <body>
        <WorkspaceShell hasSession={Boolean(cookie && viewer)} sidebar={
          <Sidebar collapsible="icon">
            <SidebarHeader>
            {name ? (
              <SidebarMenu>
                <SidebarMenuItem>
              <SidebarMenuButton render={<a href="/settings" />} size="lg" tooltip="Settings">
                <Avatar className="profile-avatar" aria-hidden="true"><AvatarFallback>{name.slice(0, 2).toUpperCase()}</AvatarFallback>{viewer?.avatar_url && <AvatarImage src={viewer.avatar_url} alt="" referrerPolicy="no-referrer" />}</Avatar>
                <span>{name} {flag(viewer?.country_code)}</span>
              </SidebarMenuButton>
                </SidebarMenuItem>
              </SidebarMenu>
            ) : (
              <SidebarMenu>
                <SidebarMenuItem>
              <SidebarMenuButton render={<a href="/login" />} size="lg" tooltip={cookie ? "Reset sign-in" : "Sign in"}>
                <Avatar className="profile-avatar profile-avatar-quiet" aria-hidden="true"><AvatarFallback>H</AvatarFallback></Avatar>
                <span>{cookie ? "Reset sign-in" : "Sign in"}</span>
              </SidebarMenuButton>
                </SidebarMenuItem>
              </SidebarMenu>
            )}
            </SidebarHeader>
            <SidebarSeparator />
            <SidebarContent>
              <SidebarGroup>
                <SidebarGroupLabel>Workspace</SidebarGroupLabel>
                <SidebarGroupContent>
                  <SidebarMenu>
                    {nav.map(([label, href, Icon]) => <SidebarMenuItem key={href}><WorkspaceNavLink href={href} label={label}><Icon /><span>{label}</span></WorkspaceNavLink></SidebarMenuItem>)}
                  </SidebarMenu>
                </SidebarGroupContent>
              </SidebarGroup>
              {name && <SidebarGroup>
                <SidebarGroupLabel>Account</SidebarGroupLabel>
                <SidebarGroupContent><SidebarMenu><SidebarMenuItem><WorkspaceNavLink href="/settings" label="Settings"><Settings /><span>Settings</span></WorkspaceNavLink></SidebarMenuItem></SidebarMenu></SidebarGroupContent>
              </SidebarGroup>}
            </SidebarContent>
            <SidebarFooter>
              <SidebarMenu>
                {viewer?.is_admin && <SidebarMenuItem><WorkspaceNavLink href="/admin" label="Admin"><ShieldCheck /><span>Admin</span></WorkspaceNavLink></SidebarMenuItem>}
                <SidebarMenuItem>{name ? <form action="/logout" method="post"><SidebarMenuButton type="submit" tooltip="Sign out"><LogOut /><span>Sign out</span></SidebarMenuButton></form> : <SidebarMenuButton render={<a href="/login" />} tooltip={cookie ? "Reset sign-in" : "Sign in"}><LogOut /><span>{cookie ? "Reset sign-in" : "Sign in"}</span></SidebarMenuButton>}</SidebarMenuItem>
              </SidebarMenu>
            </SidebarFooter>
          </Sidebar>
        }>{children}</WorkspaceShell>
      </body>
    </html>
  )
}
