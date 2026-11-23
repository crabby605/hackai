/** @type {import('next').NextConfig} */
const apiUrl = process.env.NEXT_PUBLIC_API_URL || "http://127.0.0.1:3000"

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
