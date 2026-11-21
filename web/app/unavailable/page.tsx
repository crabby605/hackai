import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export default function UnavailablePage() {
  return <section className="state-card"><p className="eyebrow">503 · Briefly unavailable</p><h1>We’re catching<br /><em>our breath.</em></h1><p>HackAI is temporarily unavailable. Your local tracker can continue collecting private features and will retry syncing later.</p><a className={cn(buttonVariants({ variant: "outline" }))} href="/dashboard">Try again</a></section>
}
