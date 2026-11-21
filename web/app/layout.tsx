import "./globals.css"
import { cookies } from "next/headers"
import { getJson } from "@/app/lib/api"
import { cn } from "@/lib/utils";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarSeparator,
} from "@/components/ui/sidebar";
import { LayoutDashboard, BarChart3, CircleDot, FolderGit2, LogOut, Settings, ShieldCheck } from "lucide-react";
import type { LucideIcon } from "lucide-react";

interface Viewer { display_name: string | null; username: string; email: string | null; country_code: string | null; avatar_url: string | null; is_admin: boolean }

function flag(country: string | null | undefined) {
  return country && /^[A-Z]{2}$/.test(country) ? String.fromCodePoint(...[...country].map((letter) => 127397 + letter.charCodeAt(0))) : ""
}

const nav: Array<[string, string, LucideIcon]> = [
  ["Overview", "/dashboard", LayoutDashboard],
  ["Projects", "/projects", FolderGit2],
  ["Struggles", "/struggles", CircleDot],
  ["Leaderboard", "/leaderboard", BarChart3],
]

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  const viewer = await getJson<Viewer>("/api/v1/me")
  const cookieStore = await cookies()
  const cookie = cookieStore.get("hackai_session")?.value
  const name = viewer?.display_name ?? viewer?.username

  // Public marketing and auth pages should not inherit the workspace chrome.
  // The sidebar is only meaningful once a session exists.
  // A stale/invalid cookie must not expose workspace navigation on public or
  // auth pages. Only a verified API viewer gets the sidebar.
  if (!cookie || !viewer) {
    return (
      <html lang="en" className={cn("dark font-sans")}>
        <body>{children}</body>
      </html>
    )
  }

  return (
    <html lang="en" className={cn("dark font-sans")}>
      <body>
        <TooltipProvider>
        <SidebarProvider>
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
              <SidebarMenuButton render={<a href={cookie ? "/logout" : "/login"} />} size="lg" tooltip={cookie ? "Reset sign-in" : "Sign in"}>
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
                    {nav.map(([label, href, Icon]) => <SidebarMenuItem key={href}><SidebarMenuButton render={<a href={href} />} tooltip={label}><Icon /><span>{label}</span></SidebarMenuButton></SidebarMenuItem>)}
                  </SidebarMenu>
                </SidebarGroupContent>
              </SidebarGroup>
              {name && <SidebarGroup>
                <SidebarGroupLabel>Account</SidebarGroupLabel>
                <SidebarGroupContent><SidebarMenu><SidebarMenuItem><SidebarMenuButton render={<a href="/settings" />} tooltip="Settings"><Settings /><span>Settings</span></SidebarMenuButton></SidebarMenuItem></SidebarMenu></SidebarGroupContent>
              </SidebarGroup>}
            </SidebarContent>
            <SidebarFooter>
              <SidebarMenu>
                {viewer?.is_admin && <SidebarMenuItem><SidebarMenuButton render={<a href="/admin" />} tooltip="Admin"><ShieldCheck /><span>Admin</span></SidebarMenuButton></SidebarMenuItem>}
                <SidebarMenuItem><SidebarMenuButton render={<a href={name ? "/logout" : cookie ? "/logout" : "/login"} />} tooltip={name ? "Sign out" : cookie ? "Reset sign-in" : "Sign in"}><LogOut /><span>{name ? "Sign out" : cookie ? "Reset sign-in" : "Sign in"}</span></SidebarMenuButton></SidebarMenuItem>
              </SidebarMenu>
            </SidebarFooter>
          </Sidebar>
          <SidebarInset className="page">{children}</SidebarInset>
        </SidebarProvider>
        </TooltipProvider>
      </body>
    </html>
  )
}
