import { NextRequest, NextResponse } from "next/server"

const protectedPaths = ["/dashboard", "/projects", "/struggles", "/archives", "/settings", "/admin", "/cli/pair"]

export function middleware(request: NextRequest) {
  const needsSession = protectedPaths.some(
    (path) => request.nextUrl.pathname === path || request.nextUrl.pathname.startsWith(`${path}/`),
  )

  if (needsSession && !request.cookies.has("hackai_session")) {
    const destination = new URL("/login", request.url)
    destination.searchParams.set("and", `${request.nextUrl.pathname}${request.nextUrl.search}`)
    return NextResponse.redirect(destination)
  }

  return NextResponse.next()
}

export const config = {
  matcher: ["/dashboard/:path*", "/projects/:path*", "/struggles/:path*", "/archives/:path*", "/settings/:path*", "/admin/:path*", "/cli/pair/:path*"],
}
