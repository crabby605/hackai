import { NextRequest, NextResponse } from "next/server"

export async function POST(request: NextRequest) {
  const origin = request.headers.get("origin")
  if (!origin || new URL(origin).origin !== request.nextUrl.origin) {
    return new NextResponse("invalid logout origin", { status: 403 })
  }
  const response = NextResponse.redirect(new URL("/login", request.url))
  const isHttps = request.nextUrl.protocol === "https:"
  for (const name of ["__Host-hackai_session", "hackai_session"]) {
    response.cookies.set(name, "", { httpOnly: true, secure: isHttps, sameSite: "lax", maxAge: 0, path: "/" })
  }
  return response
}
