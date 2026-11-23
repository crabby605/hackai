export default function BannedPage() {
  return (
    <main className="page">
      <section className="state-card">
        <p className="eyebrow">Access restricted</p>
        <h1>Your account is currently suspended.</h1>
        <p>If you think this is a mistake, contact the HackAI administrator for a review.</p>
        <form action="/logout" method="post"><button className="button button-primary" type="submit">Sign out</button></form>
      </section>
    </main>
  )
}
