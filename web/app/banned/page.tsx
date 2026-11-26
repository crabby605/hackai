import { Button } from "@/components/ui/button"

export default function BannedPage() {
  return (
    <main className="page">
      <section className="state-card">
        <p className="eyebrow">Access restricted</p>
        <h1>Your account is currently suspended.</h1>
        <p>If you think this is a mistake, contact the HackAI administrator for a review.</p>
        <form action="/logout" method="post"><Button type="submit">Sign out</Button></form>
      </section>
    </main>
  )
}
