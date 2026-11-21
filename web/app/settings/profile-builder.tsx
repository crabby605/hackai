"use client"

import { FormEvent, useEffect, useMemo, useState } from "react"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group"
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"

interface Profile { username: string; display_name: string | null; avatar_url: string | null; profile_public: boolean; leaderboard_opt_out: boolean; country_code: string | null }
type SaveState = "idle" | "saving" | "saved" | "error"
type Availability = "idle" | "checking" | "may-available" | "may-taken"

const countryCodes = [
  "AF", "AL", "DZ", "AD", "AO", "AG", "AR", "AM", "AU", "AT", "AZ", "BS", "BH", "BD", "BB", "BY", "BE", "BZ", "BJ", "BT", "BO", "BA", "BW", "BR", "BN", "BG", "BF", "BI", "CV", "KH", "CM", "CA", "CF", "TD", "CL", "CN", "CO", "KM", "CG", "CD", "CR", "CI", "HR", "CU", "CY", "CZ", "DK", "DJ", "DM", "DO", "EC", "EG", "SV", "GQ", "ER", "EE", "SZ", "ET", "FJ", "FI", "FR", "GA", "GM", "GE", "DE", "GH", "GR", "GD", "GT", "GN", "GW", "GY", "HT", "HN", "HU", "IS", "IN", "ID", "IR", "IQ", "IE", "IL", "IT", "JM", "JP", "JO", "KZ", "KE", "KI", "KP", "KR", "KW", "KG", "LA", "LV", "LB", "LS", "LR", "LY", "LI", "LT", "LU", "MG", "MW", "MY", "MV", "ML", "MT", "MH", "MR", "MU", "MX", "FM", "MD", "MC", "MN", "ME", "MA", "MZ", "MM", "NA", "NR", "NP", "NL", "NZ", "NI", "NE", "NG", "MK", "NO", "OM", "PK", "PW", "PA", "PG", "PY", "PE", "PH", "PL", "PT", "QA", "RO", "RU", "RW", "KN", "LC", "VC", "WS", "SM", "ST", "SA", "SN", "RS", "SC", "SL", "SG", "SK", "SI", "SB", "SO", "ZA", "SS", "ES", "LK", "SD", "SR", "SE", "CH", "SY", "TJ", "TZ", "TH", "TL", "TG", "TO", "TT", "TN", "TR", "TM", "TV", "UG", "UA", "AE", "GB", "US", "UY", "UZ", "VU", "VA", "VE", "VN", "YE", "ZM", "ZW",
] as const

function countryName(code: string) {
  try { return new Intl.DisplayNames(["en"], { type: "region" }).of(code) ?? code } catch { return code }
}

function localeCountry(): string {
  for (const locale of navigator.languages ?? [navigator.language]) {
    try {
      const country = new Intl.Locale(locale).region
      if (country && countryCodes.includes(country as typeof countryCodes[number])) return country
    } catch { /* Ignore malformed browser locale values. */ }
  }
  return ""
}

function usernameError(value: string): string | null {
  if (value.length < 3 || value.length > 30) return "Use 3–30 characters."
  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(value)) return "Use lowercase letters, numbers, and single hyphens only."
  return null
}

export default function ProfileBuilder({ initialProfile }: { initialProfile: Profile }) {
  const [savedProfile, setSavedProfile] = useState(initialProfile)
  const [username, setUsername] = useState(initialProfile.username)
  const [isPublic, setIsPublic] = useState(initialProfile.profile_public)
  const [leaderboardOptOut, setLeaderboardOptOut] = useState(initialProfile.leaderboard_opt_out)
  const [countryCode, setCountryCode] = useState(initialProfile.country_code ?? "")
  const [state, setState] = useState<SaveState>("idle")
  const [message, setMessage] = useState("")
  const [availability, setAvailability] = useState<Availability>("idle")
  const normalizedUsername = username.trim().toLowerCase()
  const validation = useMemo(() => usernameError(normalizedUsername), [normalizedUsername])
  const normalizedCountryCode = countryCode.trim().toUpperCase()
  const countryValid = !normalizedCountryCode || /^[A-Z]{2}$/.test(normalizedCountryCode)
  const changed = normalizedUsername !== savedProfile.username || isPublic !== savedProfile.profile_public || leaderboardOptOut !== savedProfile.leaderboard_opt_out || normalizedCountryCode !== (savedProfile.country_code ?? "")
  const profileUrl = `/profile/${encodeURIComponent(normalizedUsername || savedProfile.username)}`

  useEffect(() => {
    if (!initialProfile.country_code) setCountryCode(localeCountry())
  }, [initialProfile.country_code])

  useEffect(() => {
    if (validation || normalizedUsername === savedProfile.username) {
      setAvailability("idle")
      return
    }
    const controller = new AbortController()
    const timer = window.setTimeout(async () => {
      setAvailability("checking")
      try {
        const response = await fetch(`/api/v1/me/username-availability/${encodeURIComponent(normalizedUsername)}`, { credentials: "include", signal: controller.signal })
        if (!response.ok) throw new Error("availability unavailable")
        const body = await response.json() as { may_be_available: boolean }
        setAvailability(body.may_be_available ? "may-available" : "may-taken")
      } catch {
        if (!controller.signal.aborted) setAvailability("idle")
      }
    }, 250)
    return () => { controller.abort(); window.clearTimeout(timer) }
  }, [normalizedUsername, savedProfile.username, validation])

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (validation || !changed) return
    setState("saving")
    setMessage("")
    try {
      const response = await fetch(`/api/v1/me/profile`, {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ username: normalizedUsername, profile_public: isPublic, leaderboard_opt_out: leaderboardOptOut, country_code: normalizedCountryCode || null }),
      })
      if (response.status === 409) throw new Error("That username is already taken.")
      if (response.status === 422) throw new Error("Choose a username using lowercase letters, numbers, and hyphens.")
      if (!response.ok) throw new Error("We couldn’t save your profile. Please try again.")
      setSavedProfile((current) => ({ ...current, username: normalizedUsername, profile_public: isPublic, leaderboard_opt_out: leaderboardOptOut, country_code: normalizedCountryCode || null }))
      setState("saved")
      setMessage("Profile saved.")
      window.setTimeout(() => setState("idle"), 3500)
    } catch (error) {
      setState("error")
      setMessage(error instanceof Error ? error.message : "We couldn’t save your profile.")
    }
  }

  return (
    <section className="settings-page">
      <div className="page-heading">
        <div>
          <p className="eyebrow">Settings</p>
          <h1>Build your<br /><em>profile.</em></h1>
          <p className="heading-copy">Choose how people find you and whether your builder profile is visible outside HackAI.</p>
        </div>
      </div>

      <form className="profile-builder" onSubmit={save}>
        <Card className="settings-card">
          <CardHeader className="settings-card-heading"><div><h2>Your profile</h2><p>This is the name attached to your public profile.</p></div><Avatar className="profile-avatar profile-builder-avatar"><AvatarFallback>{(initialProfile.display_name ?? normalizedUsername).slice(0, 2).toUpperCase()}</AvatarFallback>{initialProfile.avatar_url && <AvatarImage src={initialProfile.avatar_url} alt="" referrerPolicy="no-referrer" />}</Avatar></CardHeader>
          <CardContent>
          <label className="field-label" htmlFor="username">Username</label>
          <InputGroup aria-invalid={validation ? "true" : undefined}><InputGroupAddon>hackai.dev/</InputGroupAddon><InputGroupInput id="username" name="username" value={username} onChange={(event) => { setUsername(event.target.value.toLowerCase()); setState("idle"); setMessage("") }} autoCapitalize="none" autoCorrect="off" spellCheck="false" aria-describedby="username-help" /></InputGroup>
          <p className={`field-help${validation ? " field-error" : ""}${availability === "may-available" ? " field-success" : ""}`} id="username-help">{validation ?? (availability === "checking" ? "Checking availability…" : availability === "may-available" ? "Likely available. We’ll confirm it when you save." : availability === "may-taken" ? "This name may already be in use. You can still save to confirm." : "Lowercase letters, numbers, and hyphens. You can change this later.")}</p>
          <label className="field-label profile-country-label" htmlFor="country-code">Country <span>optional</span></label>
          <NativeSelect className="w-full max-w-sm" id="country-code" name="country-code" value={countryCode} onChange={(event) => { setCountryCode(event.target.value); setState("idle"); setMessage("") }} aria-describedby="country-help">
            <NativeSelectOption value="">No country selected</NativeSelectOption>
            {countryCodes.map((code) => <NativeSelectOption key={code} value={code}>{countryName(code)}</NativeSelectOption>)}
          </NativeSelect>
          <p className={`field-help${countryValid ? "" : " field-error"}`} id="country-help">{countryCode ? "Shown as a flag next to your name. You can change it any time." : "Prefilled from your browser’s location when it is available."}</p>
          </CardContent>
        </Card>

        <Card className="settings-card">
          <CardHeader className="settings-card-heading"><div><h2>Profile visibility</h2><p>Private is the default. You decide when your profile is ready to share.</p></div></CardHeader>
          <CardContent>
          <RadioGroup className="visibility-options" value={isPublic ? "public" : "private"} onValueChange={(value) => { setIsPublic(value === "public"); setState("idle") }} aria-label="Profile visibility">
            <label className={`visibility-option${!isPublic ? " selected" : ""}`}><RadioGroupItem value="private" /><span><b>Private</b><small>Only you can view your profile and activity.</small></span></label>
            <label className={`visibility-option${isPublic ? " selected" : ""}`}><RadioGroupItem value="public" /><span><b>Public</b><small>Anyone with your link can view your profile.</small></span></label>
          </RadioGroup>
          {isPublic && <p className="public-link">Your shareable profile: <a href={profileUrl}>hackai.dev{profileUrl}</a></p>}
          </CardContent>
        </Card>

        <Card className="settings-card">
          <CardHeader className="settings-card-heading"><div><h2>Leaderboard</h2><p>Leaderboard participation is optional and never changes your credited time.</p></div></CardHeader>
          <CardContent><label className={`visibility-option${leaderboardOptOut ? " selected" : ""}`}><Checkbox checked={leaderboardOptOut} onCheckedChange={(checked) => { setLeaderboardOptOut(checked); setState("idle") }} /><span><b>Hide me from the leaderboard</b><small>Your profile can stay public while your rankings and totals remain off the leaderboard.</small></span></label></CardContent>
        </Card>

        <div className="settings-actions"><p className={`save-message ${state}`} role="status">{message}</p><Button type="submit" disabled={!changed || !!validation || !countryValid || state === "saving"}>{state === "saving" ? "Saving…" : "Save profile"}</Button></div>
      </form>
    </section>
  )
}
