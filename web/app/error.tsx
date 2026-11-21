"use client"
import { Button } from "@/components/ui/button"
export default function Error({ reset }: { error: Error & { digest?: string }; reset: () => void }) {
  return <section className="state-card"><p className="eyebrow">500 · Unexpected interruption</p><h1>That didn’t<br /><em>go to plan.</em></h1><p>No activity was lost. You can try loading this page again.</p><Button onClick={() => reset()}>Try again <span>→</span></Button></section>
}
