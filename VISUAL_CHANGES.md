# Visual Changes Guide

## Before → After Comparisons

### 🎯 Conversation Header
```
BEFORE:
┌─────────────────────────────────────────────────┐
│ Active Conversation                             │
│ My Investigation • [Badge] • [Badge]            │
│                                       2m ago     │
└─────────────────────────────────────────────────┘

AFTER:
┌─────────────────────────────────────────────────┐
│ Active Conversation                             │
│ My Investigation • [Running] • [3 jobs]  [📜][⚡][🗑] 2m ago │
└─────────────────────────────────────────────────┘
```
**Changes**: 
- Job/schedule status badges added to title row
- Action buttons (History, Compact, Delete) in header
- More compact, all info visible at once

---

### 🎯 Conversation List Item
```
BEFORE:
┌──────────────────────────────────────┐
│ ▌ Investigation: Port Scan           │  ← Thick border, heavy shadow
│   [Recipe] [MCP] [Ctx]               │
│   Completed nmap scan of 10.0.0.0/24 │  ← 2 lines preview
│                            3m ago     │
└──────────────────────────────────────┘

AFTER:
┌─────────────────────────────────────┐
│▌Investigation: Port Scan            │  ← Thinner border, softer
│ [Recipe] [MCP]                      │  ← Fewer badges
│ Completed nmap scan...      3m ago  │  ← 1 line, faster scan
└─────────────────────────────────────┘
```
**Changes**:
- 40% lighter visual weight (thinner borders/shadows)
- Single-line preview (was 2 lines)
- Smaller clip-path corners (8px → was 12px)
- Smooth spring animation on hover

---

### 🎯 Bottom Control Deck
```
BEFORE:
┌─────────────────────────────────────────────────────────────┐
│ [Compact] [History] [Delete]                                │
│                                                              │
│ Permissions: [Net off] [FS: Read ▼] [Scope: Workspace ▼]   │
│              [All off] [Reset]                              │
│                                                              │
│ tok:in=12,345 out=5,678 cost:$0.0234                       │
└─────────────────────────────────────────────────────────────┘
                    ↓↓↓

AFTER:
┌─────────────────────────────────────────────────────────────┐
│ Permissions [net:off • fs:read • scope:ws ⚙]               │
│ tok:in=12,345 out=5,678 cost:$0.0234                       │
└─────────────────────────────────────────────────────────────┘
                                                 │
                         ┌───────────────────────┘
                         ▼
              ┌─────────────────────────────────┐
              │ [Net off] [FS: Read ▼]          │  ← Popover on click
              │ [Scope: Workspace ▼]            │
              │ [All off] [Reset]               │
              └─────────────────────────────────┘
```
**Changes**:
- Compact summary chip (net/fs/scope in one line)
- Click to expand full controls in popover
- Conversation actions moved to header
- Single-row layout (was 3 rows)

---

### 🎯 Message Composer
```
BEFORE:
┌───────────────────────────────────────────────┐
│ Type instructions...                          │  84px fixed
│                                               │  height
│                                               │
│                                               │
└───────────────────────────────────────────────┘
[           Send            ]  ← Plain button

AFTER:
┌───────────────────────────────────────────────┐
│ Type instructions...                          │  52px min,
└───────────────────────────────────────────────┘  auto-grows
  ↓ (typing multi-line)
┌───────────────────────────────────────────────┐
│ I need you to scan the network                │  Expands up
│ for HTTP services and then                    │  to 50vh
│ check each one with nuclei                    │  maximum
└───────────────────────────────────────────────┘
[    ▶ SEND    ]  ← Icon + text, gradient glow on hover
```
**Changes**:
- Auto-expanding height (52px → 50vh)
- Icon + text button
- Smooth transitions
- No manual resize needed

---

### 🎯 Settings/Tools Panels
```
BEFORE:
█████████████████████████████████████████████
█                                           █  Full-screen
█  ┌────────────────────────────────┐      █  modal overlay
█  │ Settings                       │      █
█  │                                │      █
█  │ [Config form...]              │      █
█  │                                │      █
█  └────────────────────────────────┘      █
█                                           █
█████████████████████████████████████████████

AFTER:
┌─────────────────────────────┬───────────────┐
│ Conversation visible (blur) │ ┌───────────┐ │  Slide-in from
│                             │ │ Settings  │ │  right edge
│                             │ │           │ │
│                             │ │ [Config   │ │  Content stays
│                             │ │  form...] │ │  visible
│                             │ │           │ │
│                             │ └───────────┘ │
└─────────────────────────────┴───────────────┘
```
**Changes**:
- Slide-in from right (was full-screen)
- Blurred backdrop (content visible)
- Click outside to dismiss
- Faster, less disruptive

---

### 🎯 Animation Examples

#### Conversation List Hover:
```
Rest state:      →  Hover:
┌────────┐          ┌────────┐
│▌Item   │          │▌▌Item  │  ← Slides right 2px
│        │          │        │     Border brightens
└────────┘          └────────┘     Shadow increases
                    (Spring ease: cubic-bezier(0.34, 1.56, 0.64, 1))
```

#### Send Button Hover:
```
Rest:                 Hover:
┌─────────────┐       ┌─────────────┐
│  ▶ SEND     │  →    │  ▶ SEND     │  ← Lifts 2px
└─────────────┘       └─────────────┘     Yellow glow
Yellow gradient       Brighter gradient     Shadow
```

#### Permissions Popover:
```
Closed:              Opening:             Open:
[net:off • ⚙]   →   [net:off • ⚙]   →   [net:off • ⚙]
                          ▲                     ▲
                          │                     │
                     ┌────┴───┐           ┌────┴───┐
                     │ [Fade  │           │ [Full  │
                     │  in... │           │ panel] │
                     └────────┘           └────────┘
                    0.2s ease            Visible
```

---

## Color & Typography Unchanged

All core aesthetic elements preserved:
- **Yellow accent**: `#fcee0a`
- **Cyan signal**: `#00f0ff`
- **Red danger**: `#ff003c`
- **Typography**: JetBrains Mono monospace
- **Clip-path corners**: Tactical/cyberpunk style
- **Scan-line gradients**: HUD-style overlays

---

## Responsive Behavior

### Desktop (1920px):
- Full sidebar (340px)
- Slide panels (720px from right)
- Permissions popover (420px min-width)

### Tablet (960px):
- Sidebar becomes top bar
- Slide panels full width
- Permissions stack vertically

### Mobile (640px):
- Single column layout
- All panels full-screen
- Touch-friendly targets (44px min)

---

## Performance Notes

- **No layout thrashing**: All animations use `transform` and `opacity`
- **GPU acceleration**: `translate` and `clip-path` are hardware-accelerated
- **Smooth 60fps**: Cubic-bezier easing prevents jank
- **Lazy rendering**: Only visible elements animate

---

**Ready to test!** Start the Rust server and open `http://localhost:PORT` to see all improvements in action.
