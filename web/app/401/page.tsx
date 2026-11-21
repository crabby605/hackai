import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export default function UnauthorizedPage() {
  return (
    <section className="state-card">
      <p className="eyebrow">401 · Unauthorized</p>
      <h1>Authentication<br /><em>required.</em></h1>
      <p>You need to sign in with your Hack Club account to access this page.</p>
      <a className={cn(buttonVariants())} href="/login">Sign in with Hack Club <span>→</span></a>
    </section>
  )
}
