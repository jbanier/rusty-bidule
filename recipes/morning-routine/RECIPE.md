---
name: morning-routine
title: Morning Shift Handover
description: Summarize overnight CSIRT activity across collaboration, mail, calendar, and durable case memory for European shift handover.
keywords: morning, handover, catchup, catch-up, night, shift
---

Instructions:
Build an actionable handover for the last 12 hours, focused on work that may need carry-over from the US shift to the European shift.

Before writing the summary:
- Call `local__time` with `trailing_hours: 12`; use the returned UTC/local window for every source.
- Read existing investigation memory with `local__get_investigation_memory`.
- Collect Webex messages from both rooms with `local__run_skill`, `skill_name: "webex-room-conversation"`, and `tool_slug: "webex_room_message_fetch"`:
  - `CSIRT Investigators`
  - `CSIRT Analysts-Investigators On-call`
- Collect Gmail context with `local__run_skill`, `skill_name: "gmail-read"`, and `tool_slug: "gmail_read_messages"` using a query that captures recent CSIRT-relevant mail. Prefer unread or incident/security terms, and include message bodies only when snippets are not enough.
- Collect calendar context with `local__run_skill`, `skill_name: "google-calendar-read"`, and `tool_slug: "google_calendar_list_events"` using the exact time window from `local__time`.
- If a configured source fails or is not authenticated, continue with the other sources and name the gap clearly.

Write the handover as concise operator notes:
- urgent items first,
- ownership and next action when known,
- source-backed facts only,
- explicit gaps and assumptions,
- no generic status narrative.

Update investigation memory with `local__update_investigation_memory` when the handover changes durable case state, adds important entities, records a decision, or leaves unresolved questions.

Config:
  local_tools:
    - local__time
    - local__run_skill
    - local__get_investigation_memory
    - local__update_investigation_memory
    - local__search_conversation_memories

Workflow:
  type: guided_collection
  required_sources:
    - webex_rooms
    - gmail
    - google_calendar
    - investigation_memory
  output_focus: european_shift_handover

Initial Prompt:
I need a summary of the night activities. Focus only on what happened in the last 12 hours and what needs handover.

Response Template:
## {{ recipe_title }}

{{ response }}
