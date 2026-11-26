export type ErrorPageRecord = {
  eyebrow: string
  title: [string, string]
  description: string
  action: string
}

// This deliberately lives in the Next.js bundle rather than the API. Error
// screens must be able to render precisely when the API is unavailable.
export const errorPageDb = {
  notFound: [{
    eyebrow: "404 · You found nothing",
    title: ["This route", "has left the chat."],
    description: "Impressive navigation. Unfortunately, this page does not exist. Your data remains untouched.",
    action: "Retreat with dignity",
  }, {
    eyebrow: "404 · Geographic anomaly",
    title: ["You are", "somewhere else."],
    description: "This page is not here. It was never here. Stop asking questions.",
    action: "Return to civilization",
  }],
  unauthorized: [{
    eyebrow: "401 · Nice try",
    title: ["This door is", "not decorative."],
    description: "Sign in with your Hack Club account. Then we can both act like this was always the plan.",
    action: "Let me in",
  }, {
    eyebrow: "401 · Access denied",
    title: ["The robots", "said no."],
    description: "They require a Hack Club sign-in. Apparently robots have standards now.",
    action: "Negotiate with robots",
  }],
  unavailable: [{
    eyebrow: "503 · Temporarily dramatic",
    title: ["The server is", "having a moment."],
    description: "Your information is safe. The server, meanwhile, has chosen performance art.",
    action: "Check if it recovered",
  }, {
    eyebrow: "503 · System resting",
    title: ["The hamsters", "need water."],
    description: "The service is briefly offline. Your tracker will retry when the tiny wheels resume turning.",
    action: "Ask politely again",
  }],
  error: [{
    eyebrow: "500 · We fumbled it",
    title: ["That was", "deeply uncool."],
    description: "We broke the page, not your activity. Try again while we pretend this was a load test.",
    action: "Make it try again",
  }, {
    eyebrow: "500 · Skill issue",
    title: ["We made", "a computer sad."],
    description: "Your activity is safe. The page has simply developed opinions about being rendered.",
    action: "Render it again",
  }],
} as const satisfies Record<string, readonly ErrorPageRecord[]>

export function rotatingErrorPage(records: readonly ErrorPageRecord[]): ErrorPageRecord {
  return records[Math.floor(Date.now() / 30_000) % records.length] ?? records[0]
}
