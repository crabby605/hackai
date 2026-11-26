"use client"

import Link from "next/link"
import { usePathname } from "next/navigation"
import { SidebarMenuButton, useSidebar } from "@/components/ui/sidebar"

export function WorkspaceNavLink({ href, label, children }: {
  href: string
  label: string
  children: React.ReactNode
}) {
  const pathname = usePathname()
  const { setOpenMobile } = useSidebar()
  const active = pathname === href || pathname.startsWith(`${href}/`) || (href === "/dashboard" && pathname === "/")

  return <SidebarMenuButton
    render={<Link href={href} aria-current={active ? "page" : undefined} />}
    isActive={active}
    tooltip={label}
    onClick={() => setOpenMobile(false)}
  >{children}</SidebarMenuButton>
}
