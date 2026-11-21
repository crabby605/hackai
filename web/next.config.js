/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  output: "standalone",
  async rewrites() {
    return [
      { source: "/auth/:path*", destination: "http://127.0.0.1:3000/auth/:path*" },
      { source: "/callback", destination: "http://127.0.0.1:3000/callback" },
      { source: "/healthz", destination: "http://127.0.0.1:3000/healthz" },
      { source: "/api/:path*", destination: "http://127.0.0.1:3000/api/:path*" },
    ]
  },
}

module.exports = nextConfig
