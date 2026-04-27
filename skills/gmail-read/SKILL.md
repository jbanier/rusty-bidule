---
name: gmail-read
description: Reads Gmail messages from a saved read-only OAuth token. Use when the user asks to inspect inbox messages, unread mail, recent emails matching a search query, or email body text.
keywords: gmail, email, inbox, unread, messages, mail
compatibility: Requires a Google Cloud Desktop OAuth client JSON plus HTTPS access to googleapis.com.
---

# Gmail Read

Use this skill to search and read Gmail messages after a one-time local OAuth login.

Tools:
  - name: Read Gmail Messages
    slug: gmail_read_messages
    description: Read Gmail messages matching a query. Optional parameters: query, label_ids, max_results, include_body, body_max_chars.
    script: scripts/gmail_read_messages.py
    network: true
    filesystem: read_only

## When to use

- Check unread or recent inbox messages
- Read messages from a sender or topic using Gmail search syntax
- Pull subject, sender, snippet, labels, and optionally body text

## Authentication setup

1. In Google Cloud, enable the Gmail API.
2. Create an OAuth client with application type `Desktop app`.
3. Download the client JSON and either:
   - save it as `~/.config/rusty-bidule/google-oauth-client.json`, or
   - pass it explicitly with `--credentials-file`
4. Run:

```bash
python3 skills/gmail-read/scripts/gmail_auth.py login
```

Optional explicit credentials file:

```bash
python3 skills/gmail-read/scripts/gmail_auth.py login \
  --credentials-file /path/to/google-oauth-client.json
```

Saved token path defaults to `~/.config/rusty-bidule/gmail_token.json`.
Override it with `GMAIL_TOKEN_FILE=/path/to/token.json`.

## Read messages directly

Unread mail from the last week:

```bash
python3 skills/gmail-read/scripts/gmail_read_messages.py \
  --query "is:unread newer_than:7d"
```

Read with body text included:

```bash
python3 skills/gmail-read/scripts/gmail_read_messages.py \
  --query "from:alerts@example.com" \
  --include-body \
  --max-results 5
```

## Output

The script returns JSON with:

- query metadata
- `messages` array with sender, recipients, subject, date, labels, snippet
- optional `body_text` when requested

## Edge cases

- Missing token: exits with guidance to run the auth script
- Missing OAuth client JSON: exits with setup guidance
- Invalid Gmail query parameters: returns a clear validation error
- Google API failure: returns HTTP context from the failing request
