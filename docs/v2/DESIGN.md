# vibe-editor 2 GUI-first design contract

## Product thesis

vibe-editor 2 is a quiet, focus-first desktop workspace. Claude, Codex, API agents,
and vibe-team share one interaction model: choose a project, describe an outcome,
and review a durable timeline. Engine differences are capabilities, not separate UIs.

## Visual contract

- Light is the default (`#f8f8f6`); Dark uses the same DOM and dimensions (`#171716`).
- The first viewport has no permanent rail, sidebar, status bar, terminal grid, or editor tabs.
- A 48px drag region and two 44px drawer controls are the only permanent top chrome.
- The Home center contains the vibe mark, one question, and four action controls.
- Cards are allowed only because each card performs an action. They have a hairline border,
  12px radius, no shadow, and color only in their icon.
- UI and user text use Inter with Japanese platform fallbacks. Agent prose uses Source Serif 4.
- The composer is the only floating surface and the only non-modal element allowed a soft shadow.

## Interaction contract

- Composer autofocuses on launch. Enter sends, Shift+Enter inserts a newline, and IME Enter never sends.
- Quick actions insert or append prompt text and never execute automatically.
- Claude and Codex render the same component tree. Only icon, model, and capabilities differ.
- New input while a run is active is queued. Steer appears only when the runtime declares support.
- Approval, errors, tools, diffs, and tests are inline timeline events; errors never use blocking alerts.
- Terminal is compatibility-only and can be mounted only after an explicit Inspector action.
- Team is a session type. Team sessions alone expose Conversation/Canvas switching.

## Runtime boundary

Rust owns sessions, runs, event ordering, project authority, child processes, approvals,
delivery, and persistence. Renderer requests mutations with idempotency keys and receives
canonical events after pre-subscribing. Raw paths, argv, credentials, and process handles are
never accepted from Renderer as authority.

## Accessibility and performance

- WCAG 2.2 AA, 44px primary targets, visible 3px focus ring, reduced motion, and High Contrast.
- `Cmd/Ctrl+L` focuses Composer, `Cmd/Ctrl+B` opens the left drawer,
  `Cmd/Ctrl+\\` opens Inspector, and `Cmd/Ctrl+.` stops the run.
- Timeline must remain interactive with 10,000 events; runtime event projection target is p95 100ms.
- Visual baselines cover 1440x900, 1024x768, and 768x800 in Light and Dark.
