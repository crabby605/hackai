// Run against a local development server (which supplies preview data):
// PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node web/tests/ui-smoke.mjs
import assert from "node:assert/strict"
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright")
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM_PATH || undefined,
  headless: true,
})
const base = process.env.UI_BASE_URL || "http://127.0.0.1:3001"
try {
  for (const width of [375, 768, 1440]) {
    const page = await browser.newPage({ viewport: { width, height: 900 } })
    const errors = []
    page.on("pageerror", error => errors.push(error.message))
    for (const path of ["/", "/login", "/downloads", "/dashboard", "/projects", "/settings"]) {
      await page.goto(`${base}${path}`)
      await page.locator("body").waitFor()
      await page.evaluate(() => document.fonts.ready)
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `${path} overflows at ${width}px`)
      if (["/", "/login", "/downloads"].includes(path)) {
        assert.equal(await page.locator('[data-slot="sidebar-wrapper"]').count(), 0, `${path} has workspace chrome`)
      }
      if (path === "/login") {
        const colors = await page.locator('[data-slot="button"]').first().evaluate(el => {
          const style = getComputedStyle(el)
          return [style.color, style.backgroundColor]
        })
        assert.notEqual(colors[0], colors[1], "Sign-in text disappears into button background")
      }
      if (path === "/dashboard") {
        if (width < 768) {
          await page.getByRole("button", { name: "Toggle Sidebar" }).click()
          await page.getByRole("dialog").waitFor()
        }
        await page.getByRole("link", { name: "Projects", exact: true }).first().click()
        await page.waitForURL("**/projects")
        if (width < 768) {
          await page.getByRole("dialog").waitFor({ state: "hidden" })
          await page.getByRole("button", { name: "Toggle Sidebar" }).click()
        }
        assert.equal(await page.getByRole("link", { name: "Projects", exact: true }).first().getAttribute("aria-current"), "page")
        if (width < 768) {
          await page.getByRole("button", { name: "Close", exact: true }).click()
          await page.getByRole("dialog").waitFor({ state: "hidden" })
        }
      }
    }
    assert.deepEqual(errors, [], `Browser errors at ${width}px`)
    console.log(`PASS: pages, overflow, buttons, navigation at ${width}px`)
    await page.close()
  }
} finally {
  await browser.close()
}
