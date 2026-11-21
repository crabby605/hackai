export default function BannedPage() {
  return (
    <main className="page">
      <section className="state-card">
        <p className="eyebrow">Access restricted</p>
        <h1>Your account is currently suspended.</h1>
        <p>If you think this is a mistake, contact the HackAI administrator for a review.</p>
        <a className="button button-primary" href="/logout">Sign out</a>
      </section>
    </main>
  )
}
