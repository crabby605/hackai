"use client"

import { useMemo, useState } from "react"
import { Badge } from "@/components/ui/badge"
import { Input } from "@/components/ui/input"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Button } from "@/components/ui/button"

export type Account = { id: string | number; username: string; display_name: string | null; country_code: string | null; profile_public: boolean | number; leaderboard_opt_out: boolean | number; is_admin: boolean | number; projects?: number; episodes?: number; credited_seconds?: number }

export default function AccountsTable({ accounts }: { accounts: Account[] }) {
  const [query, setQuery] = useState("")
  const [pending, setPending] = useState<string | null>(null)
  const [adminState, setAdminState] = useState<Record<string, boolean>>(() => Object.fromEntries(accounts.map((account) => [String(account.id), Boolean(account.is_admin)])))
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
    <div className="review-table-wrap"><Table className="review-table"><TableHeader><TableRow><TableHead>Account</TableHead><TableHead>Visibility</TableHead><TableHead>Projects</TableHead><TableHead>Episodes</TableHead><TableHead>Credited</TableHead><TableHead /></TableRow></TableHeader><TableBody>{filtered.length ? filtered.map((account) => { const accountId = String(account.id); const isAdmin = adminState[accountId] ?? Boolean(account.is_admin); return <TableRow key={accountId}><TableCell><b>{account.display_name ?? account.username}</b><br /><small>@{account.username} {account.country_code ? `· ${account.country_code}` : ""}</small></TableCell><TableCell className="space-x-1"><Badge variant="secondary">{account.profile_public ? "Public" : "Private"}</Badge>{Boolean(account.leaderboard_opt_out) && <Badge variant="secondary">Leaderboard: Hidden</Badge>}{isAdmin && <Badge variant="outline">Admin</Badge>}</TableCell><TableCell>{account.projects ?? 0}</TableCell><TableCell>{account.episodes ?? 0}</TableCell><TableCell>{Math.round((account.credited_seconds ?? 0) / 60)} min</TableCell><TableCell className="space-x-2"><Button variant="outline" size="sm" render={<a href={`/admin/users/${encodeURIComponent(accountId)}`} />}>Open account</Button><Button variant="outline" size="sm" disabled={pending === accountId} onClick={async () => { setPending(accountId); const next = !isAdmin; const response = await fetch(`/api/v1/admin/users/${encodeURIComponent(accountId)}/admin`, { method: "POST", headers: { "Content-Type": "application/json" }, credentials: "include", body: JSON.stringify({ is_admin: next }) }); if (response.ok) setAdminState((state) => ({ ...state, [accountId]: next })); setPending(null) }}>{pending === accountId ? "Saving…" : isAdmin ? "Remove admin" : "Make admin"}</Button></TableCell></TableRow> }) : <TableRow><TableCell className="admin-empty" colSpan={6}>No accounts match “{query}”.</TableCell></TableRow>}</TableBody></Table></div>
  </>
}
