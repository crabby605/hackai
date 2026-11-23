import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export default function LandingPage() {
  return (
    <section className="landing">
      <p className="eyebrow">Build with intention</p>
      <h1>Measure the <em>hard parts</em><br />of building with AI.</h1>
      <p className="landing-copy">
        HackAI turns local coding signals into an auditable story of your
        productive struggle—without keeping prompts, tool output, or file paths.
      </p>
      <div className="hero-actions">
        <a className={cn(buttonVariants({ size: "lg" }))} href="/login">
          Start tracking
        </a>
        <a className={cn(buttonVariants({ variant: "outline", size: "lg" }))} href="/">
          See your dashboard
        </a>
      </div>
      <div className="principles">
        <article>
          <b>Local first</b>
          <p>Raw transcripts are reduced on your machine before anything can leave it.</p>
        </article>
        <article>
          <b>Evidence, not vibes</b>
          <p>Every scored hour can point back to privacy-preserving evidence.</p>
        </article>
        <article>
          <b>Built for real work</b>
          <p>Human edits, error loops, think time, and agent work are kept distinct.</p>
        </article>
      </div>
    </section>
  )
}
