import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export default function UnavailablePage() {
  return <section className="state-card"><p className="eyebrow">503 · Briefly unavailable</p><h1>We’re busy<br /><em>being unavailable.</em></h1><p>Your local tracker can keep collecting private features and will retry syncing later. It is, somehow, more reliable than this page.</p><a className={cn(buttonVariants({ variant: "outline" }))} href="/dashboard">See if it got better</a></section>
}
