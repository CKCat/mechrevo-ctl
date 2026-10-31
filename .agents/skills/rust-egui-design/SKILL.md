---
name: rust-egui-design
description: >-
  Best practices and design system guidelines for modern, accessible, and reliable Rust egui / eframe desktop applications.
  Use when designing or refactoring egui user interfaces, establishing styling hierarchies, ensuring zero-CPU idle efficiency,
  or writing robust state-driven UI code in Rust.
---

# Modern Rust egui / eframe Design & Reliability Guide

This skill provides comprehensive architectural patterns, modern design tokens, component styling conventions, and reliability guarantees for building desktop applications in Rust using `egui` and `eframe`.

---

## 1. Visual Hierarchy & Surface Elevation (Modern Dark Theme)

Modern desktop dark themes (inspired by macOS, VS Code, and Linear) must avoid flat pitch-black backgrounds and harsh borders. Instead, rely on **surface layering** and **subtle contrasting strokes**.

### Surface Hierarchy Palette
| Layer | RGB Color | Hex | Purpose |
|---|---|---|---|
| **Base / Window** | `(15, 23, 42)` / `(18, 18, 22)` | `#0f172a` / `#121216` | Main background container |
| **Panels / Sidebars** | `(24, 24, 30)` | `#18181e` | Top title bar, bottom status bar, toolbars |
| **Cards / Containers** | `(30, 41, 59)` / `(28, 30, 38)` | `#1e293b` / `#1c1e26` | Functional feature cards and group boxes |
| **Interactive Inactive** | `(40, 44, 56)` | `#282c38` | Default buttons, selector chips |
| **Interactive Hovered** | `(55, 65, 81)` | `#374151` | Hovered controls with brightness feedback |
| **Interactive Active** | `(71, 85, 105)` | `#475569` | Pressed states |
| **Subtle Border Stroke** | `(51, 65, 85)` (1.0px) | `#334155` | Outlines separating elevated cards from background |

### Accent Colors
- **Primary Cyan/Teal**: `Color32::from_rgb(6, 182, 212)` (`#06b6d4`) - Active toggles, brand highlights.
- **Power / Warning Orange**: `Color32::from_rgb(249, 115, 22)` (`#f97316`) - Fan boost, heavy power alerts.
- **Success Green**: `Color32::from_rgb(34, 197, 94)` (`#22c55e`) - AC online, success badges.
- **Destructive Red**: `Color32::from_rgb(239, 68, 68)` (`#ef4444`) - Failures, critical warnings.

---

## 2. Spacing, Geometry & Rounding Standards

Consistent spacing is critical for visual harmony and preventing cramped layouts:

- **Window / Card Rounding**:
  - Outer dialogs & main cards: `Rounding::same(8.0)` or `Rounding::same(10.0)`.
  - Buttons, chips, and input fields: `Rounding::same(6.0)`.
  - Badges / Pill tags: `Rounding::same(12.0)` (fully rounded pill).
- **Padding & Margins**:
  - Card inner margin: `egui::Margin::symmetric(14.0, 10.0)`.
  - Button padding: `egui::vec2(10.0, 5.0)` for comfortable touch and click targets.
  - Item spacing: `egui::vec2(8.0, 8.0)` default; `egui::vec2(12.0, 10.0)` for section gaps.

---

## 3. Typography & Internationalization (CJK Support)

Default egui fonts lack full CJK (Chinese, Japanese, Korean) glyph coverage.
Always configure multi-tiered fallback font definitions:

```rust
pub fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    
    // Look for system fonts in priority order:
    // 1. WenQuanYi Micro Hei / Zen Hei (/usr/share/fonts/truetype/wqy)
    // 2. Noto Sans CJK SC (/usr/share/fonts/opentype/noto)
    // 3. Microsoft YaHei / SimHei (if on dual-boot/wine paths)
    
    // Add custom font data and insert into FontFamily::Proportional
    // at the highest priority to avoid placeholder '?' boxes.
}
```

---

## 4. UI Stability & Zero Cumulative Layout Shift (0-CLS)

**Golden Rule**: *Never push or shift operational controls when dynamic notifications arrive.*

- **Fixed-Anchor Status Notifications**:
  - Do NOT insert dynamic notification banners at the top of the window that push action buttons down.
  - Pin ephemeral status messages, warnings, and success feedback to a dedicated **bottom status bar** or a floating non-intrusive toast.
- **Fixed Widths for Dynamic Metrics**:
  - Numerical telemetry (e.g., `52°C`, `105W`, `2400 RPM`) should reserve fixed text layout width or use monospace numbers to avoid jitter when digits change.

---

## 5. Architectural Reliability Patterns for egui

### Rule A: Zero GUI Blocking (Thread Decoupling)
- Never perform file I/O, D-Bus calls, ACPI/EC writes, or IPC directly inside `App::update()`.
- Use a dedicated background worker thread with `std::sync::mpsc::channel`:
  - `tx_req: Sender<IpcRequest>` sends actions from UI to worker.
  - `rx_res: Receiver<IpcResult>` delivers non-blocking results polled with `try_recv()` at the start of each frame.

### Rule B: Zero-CPU Idle Scheduling
- In immediate-mode GUI, unthrottled repainting consumes 100% of a CPU core.
- Apply proactive frame-limiting:
  ```rust
  if !window_visible {
      // Hidden in system tray: deep sleep
      ctx.request_repaint_after(Duration::from_secs(2));
  } else if auto_refresh_active {
      // Periodic telemetry polling
      ctx.request_repaint_after(Duration::from_millis(500));
  }
  // Otherwise, don't call request_repaint() -> wait for OS events
  ```

### Rule C: Testable Logic Separation
- Extract all text parsers, string formatters, and telemetry calculations into standalone pure functions outside `eframe::App`.
- Maintain 100% unit test coverage for status formatting, threshold math, and IPC protocol conversions.

