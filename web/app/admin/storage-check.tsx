"use client"

import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader } from "@/components/ui/card"

type Result = { raw_bytes: number; gzip_bytes: number; compression_ratio_permille: number; d1: { manifests: number; declared_batch_bytes: number; metadata_bytes_approx: number; compression: string } }

export default function StorageCheck() {
  const [result, setResult] = useState<Result | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [running, setRunning] = useState(false)

  async function run() {
    setRunning(true); setError(null)
    try {
      const response = await fetch("/api/v1/admin/storage-check", { method: "POST", credentials: "include" })
      if (!response.ok) throw new Error("Storage check failed")
      setResult(await response.json() as Result)
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Storage check failed") }
    finally { setRunning(false) }
  }

  return <Card className="mt-10 max-w-2xl" size="sm"><CardHeader><div><h2 className="text-base">Storage check</h2><p className="text-sm text-muted-foreground">Writes a synthetic, gzip-compressed fixture to R2. It never reads or changes user activity.</p></div></CardHeader><CardContent className="space-y-3"><Button variant="outline" onClick={run} disabled={running}>{running ? "Running…" : "Run R2 compression check"}</Button>{result && <div className="grid gap-1 text-sm text-muted-foreground"><span>R2 fixture: {result.raw_bytes.toLocaleString()} B → {result.gzip_bytes.toLocaleString()} B ({(result.compression_ratio_permille / 10).toFixed(1)}%)</span><span>D1 manifests: {result.d1.manifests} · declared payload bytes: {result.d1.declared_batch_bytes.toLocaleString()}</span><span>{result.d1.compression}</span></div>}{error && <p className="text-sm text-destructive">{error}</p>}</CardContent></Card>
}
