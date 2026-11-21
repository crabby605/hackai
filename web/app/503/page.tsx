import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export default function ServiceUnavailablePage() {
  return (
    <section className="state-card">
      <p className="eyebrow">503 · Service temporarily unavailable</p>
      <h1>Server is<br /><em>warming up.</em></h1>
      <p>HackAI is temporarily unavailable. Your information remains protected.</p>
      <a className={cn(buttonVariants({ variant: "outline" }))} href="/dashboard">Try again</a>
    </section>
  )
}
