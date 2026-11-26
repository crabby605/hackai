import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export default function UnauthorizedPage() {
  return <section className="state-card"><p className="eyebrow">401 · Credentials, please</p><h1>This bit is<br /><em>members-only.</em></h1><p>Sign in with Hack Club Auth. We need to know which dashboard is yours, shockingly.</p><a className={cn(buttonVariants())} href="/login">Prove it’s me <span>→</span></a></section>
}
