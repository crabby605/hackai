/** @type {import('next').NextConfig} */
// Rewrites run inside the Next server. They must target the private API
// origin, never the public site URL, otherwise /api requests recurse through
// the edge proxy.
const apiUrl = process.env.INTERNAL_API_URL || "http://127.0.0.1:3000"

const nextConfig = {
  reactStrictMode: true,
  output: "standalone",
  async rewrites() {
    return [
      { source: "/auth/:path*", destination: `${apiUrl}/auth/:path*` },
      { source: "/callback", destination: `${apiUrl}/callback` },
      { source: "/healthz", destination: `${apiUrl}/healthz` },
      { source: "/api/:path*", destination: `${apiUrl}/api/:path*` },
    ]
  },
}

module.exports = nextConfig
