#!/usr/bin/env python3
"""MCP stdio server: HackAI annotate tool.

Exposes a single `annotate` tool matching annotations/SKILL.md:
  - label: one of debugging|guided|delegated|reading|exploring
  - session_key: identifier for the session/window being annotated
  - evidence: pointer to transcript turn IDs and/or git hunk hashes

Calls the Rust API POST /api/v1/annotations with the Hack Club Auth session
cookie. Annotations are advisory only: they may lower/flag credit, never raise it.

Usage:
    python3 mcp/annotate/server.py
    (set HACKAI_API_BASE and HACKAI_SESSION_COOKIE env vars)
"""

import asyncio
import json
import os
import sys
from typing import Any

import httpx
from mcp.server.mcpserver import MCPServer
from mcp.types import TextContent

API_BASE = os.environ.get("HACKAI_API_BASE", "http://127.0.0.1:3000")
VALID_LABELS = {"debugging", "guided", "delegated", "reading", "exploring"}

EXPECTED_NONCE = os.environ.get("HACKAI_MCP_NONCE")
if EXPECTED_NONCE is None:
    nonce_file = os.environ.get("HACKAI_MCP_NONCE_FILE")
    if nonce_file:
        with open(nonce_file) as f:
            EXPECTED_NONCE = f.read().strip()

server = MCPServer(
    name="hackai-annotate",
    instructions=(
        "HackAI advisory annotation tool. A nonce is required on every annotate call; "
        "the agent must obtain it from the hackai install output. "
        "Call annotate with a session_key, label, evidence pointer, and nonce."
    ),
)


@server.tool()
async def annotate(
    session_key: str,
    label: str,
    evidence: str,
    nonce: str,
) -> str:
    """Report a constrained, evidence-backed productive-struggle annotation.

    Advisory only: may lower/flag credit, never raise it.
    Label must be one of: debugging, guided, delegated, reading, exploring.
    Evidence must identify transcript turn IDs and/or git hunk hashes.
    A nonce obtained from the hackai install output is required on every call.
    """
    if EXPECTED_NONCE is None:
        return json.dumps({"error": "nonce not configured"})
    if not nonce or nonce != EXPECTED_NONCE:
        return json.dumps(
            {
                "error": "nonce required",
                "note": "nonce is required on every annotate call and must match the value installed by hackai install",
            }
        )
    if label not in VALID_LABELS:
        return json.dumps(
            {
                "error": "invalid label",
                "valid_labels": sorted(VALID_LABELS),
            }
        )

    if not evidence.strip():
        return json.dumps({"error": "evidence is required"})

    session_cookie = os.environ.get("HACKAI_SESSION_COOKIE", "")

    try:
        async with httpx.AsyncClient(timeout=10.0) as client:
            headers = {"Content-Type": "application/json"}
            if session_cookie:
                headers["Cookie"] = f"hackai_session={session_cookie}"
            resp = await client.post(
                f"{API_BASE}/api/v1/annotations",
                json={
                    "session_key": session_key,
                    "label": label,
                    "evidence": evidence,
                },
                headers=headers,
            )
    except httpx.ConnectError:
        return json.dumps(
            {
                "error": "cannot reach HackAI API",
                "api_base": API_BASE,
            }
        )
    except httpx.HTTPError as exc:
        return json.dumps({"error": "request failed", "detail": str(exc)})

    if resp.status_code == 401:
        return json.dumps(
            {
                "error": "unauthorized",
                "note": "set HACKAI_SESSION_COOKIE to a valid Hack Club Auth session",
            }
        )
    if resp.status_code == 403:
        return json.dumps(
            {
                "error": "forbidden",
                "note": "admin role required for annotations",
            }
        )
    if resp.status_code == 400:
        return json.dumps(
            {
                "error": "bad request",
                "note": "label must be one of debugging/guided/delegated/reading/exploring; evidence must be non-empty",
            }
        )
    if resp.status_code != 200:
        return json.dumps(
            {
                "error": "unexpected status",
                "status": resp.status_code,
                "body": resp.text[:500],
            }
        )

    data = resp.json()
    return json.dumps(
        {
            "id": data.get("id"),
            "status": data.get("status"),
            "note": data.get("note"),
            "label": label,
            "session_key": session_key,
        }
    )


async def main() -> None:
    await server.run_stdio_async()


if __name__ == "__main__":
    asyncio.run(main())
