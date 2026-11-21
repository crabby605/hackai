"use client"

import { useMemo, useState } from "react"
import { Badge } from "@/components/ui/badge"
import { Input } from "@/components/ui/input"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { buttonVariants } from "@/components/ui/button"
import { cn } from "@/lib/utils"

export type Account = { id: string; username: string; display_name: string | null; country_code: string | null; profile_public: number; is_admin: number; projects: number; episodes: number; credited_seconds: number }

export default function AccountsTable({ accounts }: { accounts: Account[] }) {
  const [query, setQuery] = useState("")
  const [pending, setPending] = useState<string | null>(null)
  const [adminState, setAdminState] = useState<Record<string, boolean>>(() => Object.fromEntries(accounts.map((account) => [account.id, Boolean(account.is_admin)])))
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase()
    if (!needle) return accounts
    return accounts.filter((account) => [account.display_name, account.username, account.country_code].filter(Boolean).some((value) => value!.toLocaleLowerCase().includes(needle)))
  }, [accounts, query])

  return <>
    <div className="admin-search-row">
      <label className="admin-search-label" htmlFor="account-search">Search accounts</label>
      <Input id="account-search" className="admin-search" type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Name, username, or country" autoComplete="off" />
      <span className="admin-search-count">{filtered.length} of {accounts.length}</span>
    </div>
    <div className="review-table-wrap"><Table className="review-table"><TableHeader><TableRow><TableHead>Account</TableHead><TableHead>Visibility</TableHead><TableHead>Projects</TableHead><TableHead>Episodes</TableHead><TableHead>Credited</TableHead><TableHead /></TableRow></TableHeader><TableBody>{filtered.length ? filtered.map((account) => { const isAdmin = adminState[account.id] ?? Boolean(account.is_admin); return <TableRow key={account.id}><TableCell><b>{account.display_name ?? account.username}</b><br /><small>@{account.username} {account.country_code ? `· ${account.country_code}` : ""}</small></TableCell><TableCell className="space-x-1"><Badge variant="secondary">{account.profile_public ? "Public" : "Private"}</Badge>{isAdmin && <Badge variant="outline">Admin</Badge>}</TableCell><TableCell>{account.projects}</TableCell><TableCell>{account.episodes}</TableCell><TableCell>{Math.round(account.credited_seconds / 60)} min</TableCell><TableCell className="space-x-2"><a className={cn(buttonVariants({ variant: "outline", size: "sm" }))} href={`/admin/users/${encodeURIComponent(account.id)}`}>Open account</a><button className={cn(buttonVariants({ variant: "outline", size: "sm" }))} disabled={pending === account.id} onClick={async () => { setPending(account.id); const next = !isAdmin; const response = await fetch(`/api/v1/admin/users/${encodeURIComponent(account.id)}/admin`, { method: "POST", headers: { "Content-Type": "application/json" }, credentials: "include", body: JSON.stringify({ is_admin: next }) }); if (response.ok) setAdminState((state) => ({ ...state, [account.id]: next })); setPending(null) }}>{pending === account.id ? "Saving…" : isAdmin ? "Remove admin" : "Make admin"}</button></TableCell></TableRow> }) : <TableRow><TableCell className="admin-empty" colSpan={6}>No accounts match “{query}”.</TableCell></TableRow>}</TableBody></Table></div>
  </>
}
