# Frontend Testing Checklist

## 🚀 Quick Start
```bash
# Start the Rust server
cargo run

# Open browser to http://localhost:<PORT>
# (Check console for port number)
```

---

## ✅ Visual Inspection Tests

### 1. Conversation List
- [ ] Items have lighter borders/shadows than before
- [ ] Single-line preview text (not 2 lines)
- [ ] Smooth spring animation on hover (elastic bounce)
- [ ] Hover state slides item right by 2px
- [ ] Active item has yellow gradient background

### 2. Conversation Header
- [ ] Three icon buttons visible: 📜 (History), ⚡ (Compact), 🗑 (Delete)
- [ ] Job count badge appears when jobs exist
- [ ] Schedule count badge appears when schedules exist
- [ ] Badges show "running" state with cyan color
- [ ] Timestamp visible on right side

### 3. Bottom Control Deck
- [ ] Single compact permissions chip shows `net:off • fs:read • scope:ws`
- [ ] Token usage stats visible on right
- [ ] No more "Compact/History/Delete" buttons (moved to header)
- [ ] Deck is ~66% shorter than before

### 4. Permissions Popover
- [ ] Click permissions chip to open popover
- [ ] Popover appears above chip with slide-in animation
- [ ] Shows all permission controls (Net, FS, Scope, All, Reset)
- [ ] Click outside popover to close
- [ ] Press Escape to close popover
- [ ] Backdrop has blur effect

### 5. Message Composer
- [ ] Textarea starts at ~52px height
- [ ] Type multi-line text → textarea grows smoothly
- [ ] Maximum height is 50vh (half screen)
- [ ] Send button shows ▶ icon + "SEND" text
- [ ] Hover send button → lifts 2px with yellow glow
- [ ] While job running → icon disappears, shows "Working…"

### 6. Settings Panel
- [ ] Click ⚙ (settings icon) in sidebar
- [ ] Panel slides in from right edge
- [ ] Conversation remains visible with blurred backdrop
- [ ] Click backdrop to close panel
- [ ] Press Escape to close panel
- [ ] Panel slides out smoothly

### 7. Tools Panel
- [ ] Click 🧰 (tools icon) in sidebar
- [ ] Panel slides in from right edge (same as settings)
- [ ] Schedules, Evidence, Workflows visible
- [ ] Click backdrop or Escape to close

---

## 🔧 Functional Tests

### Permissions Management
1. [ ] Click permissions chip → popover opens
2. [ ] Toggle "Net off" → updates to "Net on"
3. [ ] Change filesystem dropdown → updates immediately
4. [ ] Change scope dropdown → updates immediately
5. [ ] Summary text updates to match selections
6. [ ] Click outside → popover closes
7. [ ] Reopen popover → shows current state

### Message Sending
1. [ ] Type single line → composer stays ~52px
2. [ ] Type 3-5 lines → composer grows to fit
3. [ ] Type 20+ lines → composer caps at 50vh with scrollbar
4. [ ] Press Ctrl+Enter → message sends
5. [ ] During send → button shows "Working…" (no icon)
6. [ ] After send → button returns to "▶ SEND"
7. [ ] Textarea resets to 52px height

### Conversation Actions
1. [ ] Click 📜 button → History modal opens
2. [ ] Click ⚡ button → Compaction starts
3. [ ] Click 🗑 button → Confirmation dialog appears
4. [ ] All buttons disabled when no conversation selected
5. [ ] All buttons disabled while job running

### Header Badges
1. [ ] Start a new conversation → no badges visible
2. [ ] Send message that creates job → job badge appears
3. [ ] Job badge shows "1 job running" with cyan color
4. [ ] Job completes → badge updates to "1 job"
5. [ ] Create schedule → schedule badge appears
6. [ ] Multiple jobs/schedules → count updates correctly

### Slide Panels
1. [ ] Open settings → panel slides in from right
2. [ ] Click conversation → settings visible through blur
3. [ ] Make config changes → changes work normally
4. [ ] Click backdrop → panel closes
5. [ ] Open tools → same smooth slide behavior
6. [ ] Keyboard navigation still works (Tab, Enter)

---

## 📱 Responsive Tests

### Tablet (960px width)
1. [ ] Resize browser to ~960px
2. [ ] Sidebar becomes horizontal top bar
3. [ ] Permissions popover moves to right edge
4. [ ] Slide panels become full-width
5. [ ] All buttons still accessible

### Mobile (640px width)
1. [ ] Resize to ~640px
2. [ ] Single column layout
3. [ ] Conversation list full width
4. [ ] Permissions stack vertically in popover
5. [ ] Send button full width in composer
6. [ ] Touch targets ≥44px (check with inspector)

---

## 🎨 Animation Tests

### Spring Easing
1. [ ] Hover conversation item → elastic bounce (not linear)
2. [ ] Hover send button → lift with slight overshoot
3. [ ] Permissions popover open → smooth slide with ease-out

### Smooth Scrolling
1. [ ] Send message → conversation scrolls smoothly to bottom
2. [ ] Not instant jump (should see scroll animation)
3. [ ] Works on message receive too

### Message Appearance
1. [ ] New message appears → slides in from below
2. [ ] ~0.3s animation duration
3. [ ] Slight fade-in with slide

---

## 🐛 Edge Cases

### Permissions Popover
- [ ] Open popover, open settings panel → popover closes
- [ ] Open popover, click another UI element → popover closes
- [ ] Open popover, press Escape twice → only closes popover (not settings)

### Composer Auto-Expand
- [ ] Paste 100 lines → caps at 50vh, scrollbar appears
- [ ] Delete text → shrinks back to 52px
- [ ] Resize window → max height recalculates to 50vh

### Slide Panels
- [ ] Open settings, open tools → settings closes, tools opens
- [ ] Panel open, start new conversation → panel stays open
- [ ] Panel open, delete conversation → panel closes automatically

### Header Badges
- [ ] No jobs/schedules → badges hidden (not "0 jobs")
- [ ] Job starts → badge appears immediately
- [ ] Job ends → badge updates within 5 seconds (polling interval)

---

## 🔍 Browser Compatibility

Test in multiple browsers:
- [ ] **Chrome/Chromium** 90+
- [ ] **Firefox** 88+
- [ ] **Safari** 14+
- [ ] **Edge** 90+

Check for:
- [ ] CSS `clip-path` support
- [ ] `backdrop-filter` blur works
- [ ] Cubic-bezier animations smooth
- [ ] Grid layout renders correctly

---

## ⚡ Performance Tests

### Animation Performance
1. [ ] Open DevTools → Performance tab
2. [ ] Record 5 seconds
3. [ ] Hover conversation items rapidly
4. [ ] Stop recording
5. [ ] Check FPS → should be 60fps (no jank)

### Scroll Performance
1. [ ] Send 50+ messages
2. [ ] Scroll conversation stream up/down
3. [ ] Check FPS → smooth 60fps
4. [ ] No layout thrashing warnings

### Memory Leaks
1. [ ] Open/close slide panels 20 times
2. [ ] Open DevTools → Memory tab
3. [ ] Take heap snapshot
4. [ ] Compare snapshots → no major leaks

---

## 🎯 Regression Tests

Verify existing features still work:
- [ ] Recipe application works
- [ ] MCP server toggles work
- [ ] Config save/reload works
- [ ] OAuth sign-in works
- [ ] Job tracking updates
- [ ] Schedule creation works
- [ ] Evidence bundle builds
- [ ] Workflow approval works
- [ ] Conversation deletion works
- [ ] Context compaction works

---

## 📋 Sign-Off

| Test Category | Status | Notes |
|---------------|--------|-------|
| Visual Inspection | ⬜ | |
| Functional Tests | ⬜ | |
| Responsive Tests | ⬜ | |
| Animation Tests | ⬜ | |
| Edge Cases | ⬜ | |
| Browser Compat | ⬜ | |
| Performance | ⬜ | |
| Regression | ⬜ | |

**Tested by**: _______________  
**Date**: _______________  
**Browser/OS**: _______________  
**Issues found**: _______________  

---

## 🐛 Known Issues

(Document any issues found during testing here)

---

## 🚀 Deployment Checklist

Before deploying to production:
- [ ] All tests pass
- [ ] No console errors
- [ ] No 404s for assets
- [ ] Minify CSS/JS (optional)
- [ ] Test on production-like environment
- [ ] Backup current version
- [ ] Plan rollback procedure
