import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export default function UnauthorizedPage() {
  return <section className="state-card"><p className="eyebrow">401 · Sign-in required</p><h1>This part is<br /><em>just for you.</em></h1><p>Sign in with Hack Club Auth to continue. We use it only to associate your dashboard with your account.</p><a className={cn(buttonVariants())} href="/login">Sign in <span>→</span></a></section>
}
