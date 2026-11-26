"use client"
import { Button } from "@/components/ui/button"
import { errorPageDb, rotatingErrorPage } from "@/app/lib/error-page-db"
export default function Error({ reset }: { error: Error & { digest?: string }; reset: () => void }) {
  const page = rotatingErrorPage(errorPageDb.error)
  return <section className="state-card"><p className="eyebrow">{page.eyebrow}</p><h1>{page.title[0]}<br /><em>{page.title[1]}</em></h1><p>{page.description}</p><Button onClick={() => reset()}>{page.action} <span>→</span></Button></section>
}
