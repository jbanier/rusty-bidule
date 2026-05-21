---
name: web-upload-content
description: Produces safe file upload and content handling assessment plans for MIME validation, extension filtering, storage exposure, traversal, and antivirus/error behavior.
metadata:
  keywords: web, upload, files, mime, content, traversal
---

# Web Upload And Content Handling

Use this skill to plan safe upload/download testing. It does not upload files itself; the operator must execute within an authorized test account.

Tools:
  - name: Upload Content Review
    slug: upload-content-review
    description: Generate a safe upload/download posture checklist based on allowed file types and feature notes.
    script: scripts/upload_content_review.py

