import { NextRequest, NextResponse } from "next/server"

const protectedPaths = ["/dashboard", "/projects", "/struggles", "/archives", "/settings", "/admin", "/cli/pair"]

export function middleware(request: NextRequest) {
  const needsSession = protectedPaths.some(
    (path) => request.nextUrl.pathname === path || request.nextUrl.pathname.startsWith(`${path}/`),
  )

  const session = request.cookies.get("hackai_session")?.value
  const expired = sessionExpired(session)
  if (needsSession && (!session || expired)) {
    const destination = new URL("/login", request.url)
    destination.searchParams.set("and", `${request.nextUrl.pathname}${request.nextUrl.search}`)
    if (expired) destination.searchParams.set("expired", "1")
    const response = NextResponse.redirect(destination)
    if (expired) response.cookies.delete("hackai_session")
    return response
  }

  return NextResponse.next()
}

function sessionExpired(value: string | undefined): boolean {
  if (!value) return false
  const parts = value.split(".")
  if (parts.length !== 3) return true
  try {
    const payload = JSON.parse(atob(parts[1].replace(/-/g, "+").replace(/_/g, "/"))) as { exp?: number }
    return typeof payload.exp === "number" && payload.exp <= Math.floor(Date.now() / 1000)
  } catch {
    return true
  }
}

export const config = {
  matcher: ["/dashboard/:path*", "/projects/:path*", "/struggles/:path*", "/archives/:path*", "/settings/:path*", "/admin/:path*", "/cli/pair/:path*"],
}
