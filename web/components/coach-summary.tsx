export interface CoachPattern {
  code: string
  observed_count: number
  priority: "low" | "medium" | "high" | string
  reason_codes: string[]
  tip: string
}

export interface CoachSummaryData {
  data_source: string
  recent_derived_events: number
  baseline_weekly_derived_events: number | null
  patterns: CoachPattern[]
}

const patternNames: Record<string, string> = {
  failure_without_grounding: "Add context to failure reports",
  approved_after_red_verification: "Verify before approving",
  repeated_loop: "Break repeated loops sooner",
  fullcaps_unanchored_signal: "Ground urgent requests",
}

function patternName(pattern: CoachPattern) {
  return patternNames[pattern.code] ?? "Derived collaboration pattern"
}

export function CoachSummary({ coach, compact = false }: { coach: CoachSummaryData | null; compact?: boolean }) {
  if (!coach) return null

  if (coach.patterns.length === 0) {
    return (
      <section className={`coach-summary ${compact ? "coach-summary-compact" : ""}`} aria-labelledby="coach-heading">
        <div className="coach-heading">
          <div>
            <p className="eyebrow">Coaching</p>
            <h2 id="coach-heading">No recurring patterns yet</h2>
          </div>
        </div>
        <p className="coach-empty">We did not find a repeated rule-based pattern in your last seven days of derived activity.</p>
      </section>
    )
  }

  return (
    <section className={`coach-summary ${compact ? "coach-summary-compact" : ""}`} aria-labelledby="coach-heading">
      <div className="coach-heading">
        <div>
          <p className="eyebrow">Coaching</p>
          <h2 id="coach-heading">Small adjustments for your next session</h2>
          <p>Rule-based suggestions from {coach.recent_derived_events} derived event{coach.recent_derived_events === 1 ? "" : "s"} in the last seven days.</p>
        </div>
      </div>
      <div className="coach-patterns">
        {coach.patterns.map((pattern) => (
          <article className="coach-pattern" key={pattern.code}>
            <div className="coach-pattern-meta">
              <span className={`coach-priority coach-priority-${pattern.priority}`}>{pattern.priority} priority</span>
              <span>{pattern.observed_count} observed</span>
            </div>
            <h3>{patternName(pattern)}</h3>
            <p>{pattern.tip}</p>
          </article>
        ))}
      </div>
      <p className="coach-privacy-note">Suggestions use opt-in derived records. Prompt text and tool output are not displayed.</p>
    </section>
  )
}
