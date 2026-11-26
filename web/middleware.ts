import { NextRequest, NextResponse } from "next/server"

const protectedPaths = ["/dashboard", "/projects", "/struggles", "/evidence", "/settings", "/admin", "/cli/pair"]

export function middleware(request: NextRequest) {
  const isLocalPreview = process.env.NODE_ENV === "development"
    && (request.nextUrl.hostname === "127.0.0.1" || request.nextUrl.hostname === "localhost")
  if (isLocalPreview) return NextResponse.next()

  const needsSession = protectedPaths.some(
    (path) => request.nextUrl.pathname === path || request.nextUrl.pathname.startsWith(`${path}/`),
  )

  const session =
    request.cookies.get("__Host-hackai_session")?.value ??
    request.cookies.get("hackai_session")?.value
  // The browser cannot validate a signed session token.  Treat the cookie as an
  // opaque credential and let the API verify its signature and expiry.
  if (needsSession && !session) {
    const destination = new URL("/login", request.url)
    destination.searchParams.set("and", `${request.nextUrl.pathname}${request.nextUrl.search}`)
    return NextResponse.redirect(destination)
  }

  return NextResponse.next()
}

export const config = {
  matcher: ["/dashboard/:path*", "/projects/:path*", "/struggles/:path*", "/evidence/:path*", "/settings/:path*", "/admin/:path*", "/cli/pair/:path*"],
}
