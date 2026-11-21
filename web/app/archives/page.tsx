import ArchiveBrowser from "@/components/archive-browser"

export default function ArchivesPage() {
  return <section className="dashboard"><div className="dashboard-heading"><div><p className="eyebrow">Archive</p><h1>Your sync<br /><em>archives.</em></h1><p className="heading-copy">Locate the R2 batches behind your tracked work.</p></div></div><ArchiveBrowser /></section>
}
