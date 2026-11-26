import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { errorPageDb, rotatingErrorPage } from "@/app/lib/error-page-db"

export default function UnauthorizedPage() {
  const page = rotatingErrorPage(errorPageDb.unauthorized)
  return (
    <section className="state-card">
      <p className="eyebrow">{page.eyebrow}</p>
      <h1>{page.title[0]}<br /><em>{page.title[1]}</em></h1>
      <p>{page.description}</p>
      <a className={cn(buttonVariants())} href="/login">{page.action} <span>→</span></a>
    </section>
  )
}
