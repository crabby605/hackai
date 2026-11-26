"use client"

import { usePathname } from "next/navigation"
import { SidebarInset, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar"
import { TooltipProvider } from "@/components/ui/tooltip"

export function WorkspaceShell({ children, sidebar, hasSession }: {
  children: React.ReactNode
  sidebar: React.ReactNode
  hasSession: boolean
}) {
  const pathname = usePathname()
  const publicPage = ["/login", "/landing", "/downloads", "/cli/pair", "/401", "/503", "/banned", "/unauthorized", "/unavailable"].includes(pathname)
    || (pathname === "/" && !hasSession)

  if (publicPage) return <div className="public-shell">{children}</div>

  return <TooltipProvider>
    <SidebarProvider>
      {sidebar}
      <SidebarInset className="min-w-0">
        <header className="flex h-14 shrink-0 items-center gap-3 border-b px-4">
          <SidebarTrigger />
          <span className="text-sm font-medium">HackAI</span>
        </header>
        <div className="page">{children}</div>
      </SidebarInset>
    </SidebarProvider>
  </TooltipProvider>
}
