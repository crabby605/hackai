import { cookies } from "next/headers"
import DashboardPage from "./dashboard/page"
import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { getJson } from "@/app/lib/api"

export default function LandingPage() {
  // `/` is the workspace for a signed-in builder and the public landing page
  // for everyone else.
  return <Home />
}

async function Home() {
  const cookieStore = await cookies()
  const cookie = cookieStore.get("hackai_session")?.value
  if (cookie && await getJson("/api/v1/me")) return <DashboardPage />

  return (
    <section className="landing">
      <header className="marketing-nav">
        <a className="marketing-brand" href="/">Hack<span>AI</span></a>
        <nav aria-label="Marketing navigation">
          <a href="#why">Philosophy</a>
          <a href="#features">Features</a>
          <a href="#privacy">Privacy</a>
          <a href="/login" className={cn(buttonVariants({ size: "sm" }), "marketing-signin")}>Sign in</a>
        </nav>
      </header>

      <div className="landing-hero">
        <h1>Build with AI.<br />Know what it <span>takes.</span></h1>
        <p className="landing-copy">Understand how effectively you use AI: track focused time, collaboration mix, and the sessions that need another pass.</p>
        <div className="hero-actions">
          <a className={cn(buttonVariants({ size: "lg" }))} href="/login">Open dashboard <span aria-hidden="true">→</span></a>
        </div>
      </div>

      <div className="landing-preview" aria-label="HackAI dashboard preview">
        <div className="preview-topbar"><i></i><i></i><i></i></div>
        <div className="preview-body">
          <aside><b>Hack<span>AI</span></b><small>Overview</small><small>Projects</small><small>Struggles</small></aside>
          <div className="preview-content"><em>Workspace diagnostics</em><h2>Find the <span>work that needs attention.</span></h2><div className="preview-metrics"><b>4.6h<small>Credited today</small></b><b>72%<small>Human contribution</small></b><b>3<small>Open loops</small></b></div></div>
        </div>
      </div>

      <div className="landing-principles" id="features">
        <article id="why"><b>Diagnose friction</b><p>Find repeated attempts, unresolved work, and patterns worth fixing.</p></article>
        <article><b>Separate the signals</b><p>Understand the contribution of human work and AI-assisted work.</p></article>
        <article id="privacy"><b>Keep control</b><p>Raw prompts, tool output, and file paths stay out of your dashboard.</p></article>
      </div>
    </section>
  )
}
