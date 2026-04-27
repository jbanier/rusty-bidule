---
name: splunk
description: Uses advertised MCP tools for Splunk search, job submission, and result retrieval. Use when the user wants to search Splunk, enumerate indexes or sourcetypes, inspect events, run SPL, or follow up on long-running Splunk jobs.
metadata:
  keywords: splunk, spl, search, index, sourcetype, events, sid, job, logs, siem
---

# Splunk Investigation Via MCP

## Overview

Use this skill when the task requires querying Splunk through MCP rather than a local CLI script.

Prefer this skill for:

- running SPL searches
- enumerating active indexes or sourcetypes
- retrieving recent events for investigation
- handling Splunk search jobs that may outlive a single turn
- resuming a search by Splunk search ID / SID

This skill is MCP-backed. Use the Splunk-related MCP tools advertised in the current session. Do not claim the skill is locally executable.

## Tool selection

Start by identifying the Splunk-related tools exposed in the current tool list. Prefer tools that map to these actions:

- submit or run a search
- poll or inspect a search job by SID
- fetch search results or events
- optionally cancel a search job

If multiple Splunk tools exist, prefer the most specific one over a generic wrapper.

## Search strategy

- Always time-bound searches unless the user explicitly asks for a different window.
- Prefer narrow queries and result limits first, then widen only if needed.
- For exploratory work, start with small `head`/`limit` patterns or recent windows.
- If the user asks for an index overview, use `tstats` or metadata-style searches when available instead of broad raw scans.
- If a search is likely to be expensive, prefer async submission and later retrieval over blocking the turn.

## Safe default patterns

Use these SPL patterns as guidance when forming searches:

Enumerate active indexes:

```spl
| tstats count where index=* earliest=-15m@m latest=now by index | sort -count
```

Enumerate sourcetypes inside an index:

```spl
| tstats count where index=<index> earliest=-15m@m latest=now by sourcetype | sort -count
```

Sample recent events for a sourcetype:

```spl
index=<index> sourcetype=<sourcetype> earliest=-15m@m latest=now | head 20
```

Targeted event retrieval:

```spl
index=<index> <constraints> earliest=-24h latest=now | sort 0 - _time | head 100
```

## Long-running jobs

If the MCP tool returns a Splunk SID or other remote job identifier instead of immediate results:

1. Treat that as a long-running search job, not a failure.
2. Store it with `local__remember_job`.
3. Use:
   - `source_tool`: the Splunk MCP tool name
   - `status`: `running` or the closest returned status
   - `mode`: `auto_pull`
   - `transaction_id`: the SID / search ID
   - `poll_interval_seconds`: typically `30`
   - `retrieval_state`: short state such as `submitted` or `polling`
   - `automation_prompt`: tell the agent to poll the Splunk job, retrieve results when ready, and summarize them
4. If a follow-up call is needed in the same turn, poll once immediately when reasonable.
5. If the search is still running, tell the user it was deferred and include the stored alias and SID.

Suggested alias format:

```text
splunk-<short-purpose>-<sid-suffix>
```

## Result handling

When results are available:

- show the key records first
- mention the exact SPL or an equivalent summary of the query
- include the time window used
- say clearly if the results are truncated
- separate raw findings from interpretation

If the result set is large, summarize the most relevant rows and explain what was omitted.

## Failure handling

- If Splunk returns a timeout but also provides a SID, store the job and continue asynchronously.
- If Splunk returns no SID and no results, report the failure plainly and preserve any error text.
- If the user asks to rerun a costly search, narrow the time range or add constraints first unless they explicitly want the broad query.
- Do not claim “no evidence” when the real outcome is “query still running” or “job retrieval failed”.

## Output expectations

For Splunk-backed answers, prefer this shape:

- `Results`: the returned events, counts, or most relevant rows
- `Conclusion`: concise interpretation
- `Sources`: `<mcp-server> / <tool-name> -> what it returned`

For async jobs, include:

- stored alias
- SID / transaction ID
- current status
- whether auto-pull is configured

## MCP metadata

Tools:
  - name: Submit Splunk Search
    slug: submit-search
    mcp: true
    description: Submit or run a Splunk SPL query through an advertised MCP server.
  - name: Poll Splunk Job
    slug: poll-job
    mcp: true
    description: Check the status of a previously submitted Splunk search job by SID.
  - name: Fetch Splunk Results
    slug: fetch-results
    mcp: true
    description: Retrieve results for a completed or partially completed Splunk search job.
