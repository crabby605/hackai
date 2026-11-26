import { Plugin } from "@opencode/plugin"

const send = async (event: string, payload: unknown) => {
  const proc = Bun.spawn(["hackai", "harness", "hook", "--repo", ".", "--harness", "opencode", "--event", event], {
    stdin: "pipe", stdout: "ignore", stderr: "ignore",
  })
  proc.stdin.write(JSON.stringify(payload))
  proc.stdin.end()
  await proc.exited
}

export default Plugin.define({
  id: "hackai.local-telemetry",
  async setup(ctx) {
    await ctx.session.hook("prompt", (event) => send("prompt", { sessionId: event.sessionID, prompt: event.text }))
    await ctx.tool.hook("execute.before", (event) => send("tool-before", event))
    await ctx.tool.hook("execute.after", (event) => send("tool-after", event))
    await ctx.permission.hook("evaluate", (event) => send("permission", event))
    await ctx.session.hook("retry", (event) => send("retry", event))
  },
})
