# LocalView UI Architecture — Nolane-UI-Intelligence Route

This document is the implementation contract for the desktop surface. It is intentionally narrower than the product specification: it governs **how the very large LocalView capability set is exposed without turning LocalView into a cluttered IDE**.

## Governing route

The UI wave follows the Nolane-UI-Intelligence lifecycle rather than treating it as a style reference:

1. `using-nolane-ui` — activate the skill system and evidence discipline.
2. `nolane-ui` — route the flagship desktop task.
3. `designing-desktop-windowed-workspaces` — establish the main-region / overlay / modal / launcher contract.
4. LocalView Product Spec — preserve the product's zero-clutter, native-feeling, Human View / X-Ray / command-palette model.
5. Implementation — React surface inside the Tauri shell, backed by Rust runtime state.
6. Verification — frontend production build, Rust check/clippy/tests, then rendered UI critique once a GUI runner is available.

## Chosen flagship direction

**Quiet Instrument Surface**

The localhost application is the product surface. LocalView itself is mostly absent until requested.

### Region contract

| Region | Default state | Purpose | May resize the workspace? |
| --- | --- | --- | --- |
| Main workspace | Always visible | Actual localhost application | N/A |
| Floating left rail | Visible, compact | Tool launcher | No |
| Floating target pill | Visible, compact | Session identity + runtime actions | No |
| Inspector / X-Ray | Hidden | Structure, geometry, source mapping | No |
| Responsive Lab | Hidden | Viewport / breakpoint analysis | No |
| Console / Network | Hidden | Telemetry sheets | No |
| AI Critic | Hidden | Grounded visual diagnosis | No |
| Command palette | Hidden | Keyboard-first access to all capabilities | No |

No persistent sidebar, status dashboard, capability grid, topology diagram, or console strip is allowed in the normal workspace.

## Interaction laws

- `Esc` closes the active surface.
- `Cmd/Ctrl + K` opens the command palette.
- `I`, `R`, `C`, `N`, `A` open the corresponding tool when focus is not inside an editable control.
- Clicking an already-active rail item closes it.
- Floating surfaces never change iframe/native-preview geometry.
- Disconnected sessions may cover the target with a reconnect-state veil, but the tool chrome stays usable.
- The session selector remains port-independent at the project identity layer.
- “Immersive” mode fades chrome instead of destroying tool state.

## Visual system

- Dark neutral optical shell to avoid fighting the user's application.
- One cool accent reserved for selection, active tools, and AI/X-Ray affordances.
- Thin optical borders, moderate translucent surfaces, layered shadow, small radii.
- Dense information is contained inside temporary surfaces, never on the main canvas.
- Micro labels use uppercase tracking only for system taxonomy, not body copy.
- Motion is short (roughly 140–180 ms), reversible, and disabled under `prefers-reduced-motion`.
- Full keyboard focus rings are mandatory.

## Truthfulness rules

The UI must not fabricate runtime evidence. If a Rust analyzer exists but the secure live bridge is not connected yet, the surface says so explicitly. “Empty but true” is preferred over demo data.

## Progressive disclosure model

LocalView exposes capability depth in this order:

1. **Human View:** application only.
2. **Tool affordance:** small floating rail.
3. **Focused tool:** one overlay or sheet.
4. **X-Ray evidence:** semantic refs, geometry, source mapping.
5. **AI interpretation:** grounded in the available evidence packet.
6. **Heavy engine escalation:** Chromium only when browser-specific evidence is necessary.

This mirrors the runtime engine strategy: heavy UI and heavy browser machinery are both exceptional paths, not defaults.
