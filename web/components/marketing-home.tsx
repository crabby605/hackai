import { ArrowRight, ArrowUpRight, Braces, ChartNoAxesCombined, CodeXml, Command, Fingerprint, GitBranch, Sparkles, Terminal } from "lucide-react"
import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

const harnesses = [
  { name: "Claude Code", icon: Sparkles, detail: "Local session history" },
  { name: "Codex", icon: Command, detail: "Local session history" },
  { name: "Gemini CLI", icon: Braces, detail: "Local session history" },
  { name: "OpenCode", icon: CodeXml, detail: "Local session database" },
  { name: "Aider", icon: Terminal, detail: "Conversation history import" },
]

const features = [
  { icon: ChartNoAxesCombined, title: "See the work behind the output.", text: "Bring recorded sessions together by project. Review human activity, agent time, and the iterations along the way." },
  { icon: GitBranch, title: "Follow the process.", text: "Explore derived session summaries to see how you directed, revised, and verified your work." },
  { icon: Fingerprint, title: "Keep your work yours.", text: "Raw prompts and tool output stay on your device. Dashboard sync is opt-in and uses derived records." },
]

export function MarketingHome() {
  const primary = cn(buttonVariants({ size: "lg" }), "min-h-11 px-5")
  const secondary = cn(buttonVariants({ variant: "outline", size: "lg" }), "min-h-11 px-5")

  return <div className="w-full min-w-0 bg-[#0a0a0a] text-[#f5f5f5]">
    <a href="#main-content" className="sr-only focus:not-sr-only focus:absolute focus:z-50 focus:bg-background focus:p-4">Skip to content</a>
    <header className="mx-auto flex min-h-20 max-w-6xl flex-wrap items-center justify-between gap-4 border-b border-white/10 px-5 py-4 sm:px-8">
      <a href="/landing" aria-label="HackAI home" className="text-xl font-semibold tracking-tight">Hack<span className="text-neutral-400">AI</span><span className="ml-1 text-neutral-600">_</span></a>
      <nav aria-label="Main navigation" className="flex items-center gap-4 text-sm sm:gap-7">
        <a href="#harnesses" className="text-neutral-400 transition-colors hover:text-white">Harnesses</a>
        <a href="/downloads" className="hidden text-neutral-400 transition-colors hover:text-white sm:inline-flex">Downloads</a>
        <a href="/login" className={cn(buttonVariants({ variant: "outline" }), "min-h-10 px-4")}>Sign in <ArrowUpRight aria-hidden="true" /></a>
      </nav>
    </header>

    <main id="main-content" className="mx-auto max-w-6xl px-5 sm:px-8">
      <section className="py-16 sm:py-24" aria-labelledby="hero-heading">
        <p className="mb-6 flex items-center gap-2 text-xs font-medium tracking-widest text-neutral-400 uppercase"><span className="size-1.5 rounded-full bg-neutral-300" aria-hidden="true" /> Your work. Your perspective.</p>
        <h1 id="hero-heading" className="max-w-4xl text-[clamp(2.5rem,6.5vw,5.5rem)] leading-[1.1] font-semibold tracking-tight">Build with AI.<br /><span className="text-neutral-400">Know what it takes.</span></h1>
        <p className="mt-6 max-w-xl text-base leading-8 text-neutral-400 sm:text-lg">A clearer picture of your AI-assisted work. Bring your sessions together, follow your progress, and keep your raw conversations on your machine.</p>
        <div className="mt-8 flex flex-wrap gap-3">
          <a href="/downloads" className={primary}>Start tracking <ArrowRight aria-hidden="true" /></a>
          <a href="/login" className={secondary}>Open dashboard</a>
        </div>
        <p className="mt-4 text-xs leading-6 text-neutral-500">Local collection · Opt-in sync · Your choice of harness</p>
      </section>

      <section aria-label="Illustrative dashboard preview" className="overflow-hidden rounded-xl border border-white/15 bg-[#111]">
        <div className="flex items-center justify-between gap-4 border-b border-white/10 px-5 py-3 text-xs text-neutral-500">
          <span className="flex gap-1.5" aria-hidden="true"><span className="size-2 rounded-full bg-neutral-500" /><span className="size-2 rounded-full bg-neutral-700" /><span className="size-2 rounded-full bg-neutral-700" /></span>
          <span>Workspace preview · Example data</span>
        </div>
        <div className="grid sm:grid-cols-[150px_minmax(0,1fr)]">
          <div aria-hidden="true" className="hidden border-r border-white/10 p-5 text-xs text-neutral-500 sm:block"><b className="mb-8 block text-base text-neutral-200">HackAI</b><span className="mb-4 block rounded bg-white/5 px-3 py-2 text-neutral-200">Overview</span><span className="mb-5 block px-3">Projects</span><span className="block px-3">Struggles</span></div>
          <div className="min-w-0 p-5 sm:p-8">
            <p className="text-xs text-neutral-500">Your workspace, at a glance</p>
            <h2 className="mt-2 text-xl font-medium sm:text-2xl">The work behind the build.</h2>
            <div className="mt-7 grid grid-cols-3 gap-3 border-y border-white/10 py-5 sm:gap-6">
              {[['4.6h', 'Recorded time'], ['3', 'Projects'], ['12', 'Sessions']].map(([value, label]) => <div key={label}><strong className="block text-xl font-medium sm:text-3xl">{value}</strong><span className="mt-2 block text-[11px] text-neutral-500 sm:text-xs">{label}</span></div>)}
            </div>
            <div className="mt-6 flex h-16 items-end gap-2" aria-hidden="true">{[25, 42, 32, 65, 48, 80, 55, 70, 95, 62, 78, 88].map((height, i) => <span key={i} className="flex-1 rounded-t bg-neutral-600 last:bg-neutral-200" style={{ height: `${height}%` }} />)}</div>
          </div>
        </div>
      </section>

      <section id="features" aria-label="Why HackAI" className="grid scroll-mt-8 gap-8 py-16 sm:grid-cols-3 sm:gap-10 sm:py-20">
        {features.map(({ icon: Icon, title, text }) => <article key={title}><Icon className="mb-5 size-5 text-neutral-400" aria-hidden="true" /><h2 className="text-base font-medium leading-7">{title}</h2><p className="mt-3 text-sm leading-7 text-neutral-400">{text}</p></article>)}
      </section>

      <section id="harnesses" aria-labelledby="harness-heading" className="scroll-mt-8 border-y border-white/10 py-14 sm:py-20">
        <div className="max-w-2xl"><p className="mb-4 text-xs tracking-widest text-neutral-500 uppercase">Fits your workflow</p><h2 id="harness-heading" className="text-3xl font-semibold tracking-tight sm:text-4xl">Works with your harness.</h2><p className="mt-5 text-base leading-8 text-neutral-400">Keep the tools you already build with. HackAI can collect or import local session history from these coding agents.</p></div>
        <ul className="mt-9 grid list-none grid-cols-1 gap-3 p-0 min-[400px]:grid-cols-2 lg:grid-cols-5">
          {harnesses.map(({ name, icon: Icon, detail }) => <li key={name}><a href="/downloads#setup" aria-label={`${name}: view CLI setup`} className="group flex h-full flex-col gap-5 rounded-lg border border-white/10 bg-[#111] p-5 transition-colors hover:border-white/30 hover:bg-[#171717] focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-white"><div className="flex items-center justify-between"><Icon className="size-6 text-neutral-300" aria-hidden="true" /><ArrowUpRight className="size-4 text-neutral-600 group-hover:text-neutral-300" aria-hidden="true" /></div><div><h3 className="text-sm font-medium">{name}</h3><p className="mt-2 text-xs leading-5 text-neutral-500">{detail}</p></div></a></li>)}
        </ul>
        <p className="mt-5 max-w-2xl text-xs leading-6 text-neutral-500">Available signals vary by harness. Aider history preserves conversation order but does not provide precise per-turn timing.</p>
      </section>

      <section aria-labelledby="start-heading" className="flex flex-col items-start justify-between gap-7 py-16 sm:flex-row sm:items-center sm:py-20"><div><h2 id="start-heading" className="text-2xl font-semibold sm:text-3xl">Your next session, in perspective.</h2><p className="mt-3 text-sm leading-7 text-neutral-400">Install the CLI, choose a project, and start collecting locally.</p></div><a href="/downloads" className={primary}>Get HackAI <ArrowRight aria-hidden="true" /></a></section>
    </main>
    <footer className="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-4 border-t border-white/10 px-5 py-7 text-xs text-neutral-500 sm:px-8"><span>HackAI · Built for the process.</span><div className="flex gap-6"><a href="#harnesses" className="hover:text-white">Harnesses</a><a href="/downloads" className="hover:text-white">CLI setup</a><a href="/login" className="hover:text-white">Sign in</a></div></footer>
  </div>
}
