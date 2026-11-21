import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export default function NotFound() {
  return <section className="state-card"><p className="eyebrow">404 · Lost in the build</p><h1>This route<br /><em>doesn’t exist.</em></h1><p>Nothing was changed and no tracking data was touched.</p><a className={cn(buttonVariants())} href="/">Back to HackAI <span>→</span></a></section>
}
