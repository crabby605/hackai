import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Check, Download, Terminal } from "lucide-react"

const installer = "https://github.com/crabby605/hackai/releases/latest/download/install-cli.sh"
const releases = "https://github.com/crabby605/hackai/releases/latest"

function Command({ children }: { children: string }) {
  return <pre className="download-command"><code>{children}</code></pre>
}

export default function DownloadsPage() {
  return <main className="downloads-page">
    <header className="marketing-nav">
      <a className="marketing-brand" href="/">Hack<span>AI</span></a>
      <nav aria-label="Marketing navigation">
        <a href="/">Home</a>
        <a href="/downloads" aria-current="page">Downloads</a>
        <a href="/login" className="marketing-signin">Sign in</a>
      </nav>
    </header>

    <section className="downloads-hero">
      <Badge variant="outline">Local-first CLI</Badge>
      <h1>Start tracking<br /><em>in a minute.</em></h1>
      <p>HackAI runs on your machine, turns local harness activity into private derived records, and only syncs after you choose dashboard tracking.</p>
    </section>

    <section className="download-grid" aria-label="Available downloads">
      <Card>
        <CardHeader>
          <div className="download-icon"><Download /></div>
          <CardTitle>macOS</CardTitle>
          <CardDescription>Apple Silicon and Intel</CardDescription>
        </CardHeader>
        <CardContent>
          <Command>{`curl -fsSL ${installer} | bash`}</Command>
          <p>Installs the matching release binary to your PATH and verifies its checksum.</p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <div className="download-icon"><Terminal /></div>
          <CardTitle>Linux</CardTitle>
          <CardDescription>x86_64 and ARM64</CardDescription>
        </CardHeader>
        <CardContent>
          <Command>{`curl -fsSL ${installer} | bash`}</Command>
          <p>Uses the same installer and selects the matching architecture.</p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <div className="download-icon"><Download /></div>
          <CardTitle>Windows</CardTitle>
          <CardDescription>Manual binary download</CardDescription>
        </CardHeader>
        <CardContent>
          <a className="download-link" href={releases}>Open latest release</a>
          <p>Download <code>hackai-windows-x86_64.exe</code>, then add its folder to PATH.</p>
        </CardContent>
      </Card>
    </section>

    <section id="setup" className="setup-guide scroll-mt-8" aria-labelledby="setup-heading">
      <div><p className="eyebrow">Setup</p><h2 id="setup-heading">Track a project.</h2><p>Each project keeps its rich transcript classification locally. Prompt and reply text, code, paths, and tool output do not get uploaded.</p></div>
      <ol>
        <li><span>1</span><div><b>Authorize once</b><Command>hackai login</Command><p>Your system Keychain stores one credential for all tracked projects.</p></div></li>
        <li><span>2</span><div><b>Track a folder</b><Command>cd your-project && hackai track --dashboard</Command><p>Installs supported harness adapters, performs initial local collection, and starts background sync.</p></div></li>
        <li><span>3</span><div><b>Open your workspace</b><Command>hackai dash</Command><p>See local sessions immediately; use the web dashboard for synced project trends and evidence.</p></div></li>
      </ol>
    </section>

    <section className="download-notes">
      <p><Check /> Supports Codex, Claude Code, OpenCode, and Antigravity automatic adapters.</p>
      <p><Check /> Gemini and Aider can be imported with lower-confidence timing.</p>
      <p><Check /> Run <code>hackai update</code> to check for new releases.</p>
    </section>
  </main>
}
