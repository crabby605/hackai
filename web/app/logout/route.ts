import { NextRequest, NextResponse } from "next/server"

export async function GET(request: NextRequest) {
  const response = NextResponse.redirect(new URL("/login", request.url))
  response.cookies.set("hackai_session", "", { httpOnly: true, secure: true, sameSite: "lax", maxAge: 0, path: "/" })
  return response
}
