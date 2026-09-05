# Compact Tool Header Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse the full-width `ToolHeader` card + separate controls row on all nine tools into one slim header row with an icon-only About button and an inline controls slot.

**Architecture:** One shared-component change in `tool_help.rs` (drop the panel card, shrink the h1, icon-only ⓘ, add an optional right-aligned `children` slot), then a mechanical migration at each of the nine call sites moving the page's existing controls row into the slot. The expandable about-panel and its i18n strings are untouched.

**Tech Stack:** Rust / Leptos 0.8 (`view!` macro), Tailwind classes, leptos-i18n.

## Global Constraints

- No new user-facing strings; reuse `tool_help_about_tool` / `tool_help_hide_info` as `aria-label`/`title`.
- The header row must stay OUTSIDE every Suspense/Transition boundary at all call sites (world picker must survive loads). Suspense-wrapped status text *inside* the controls slot is fine — it's there today.
- Spec: `docs/superpowers/specs/2026-08-29-compact-tool-header-design.md`.
- Before any commit: `./check_ci.sh` per CLAUDE.md (`REAL_EXIT` check, not piped `$?`).

---

### Task 1: Rework `ToolHeader`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/tool_help.rs:8-63`

**Interfaces:**
- Produces: `ToolHeader` with a new optional `children: Option<Children>` prop; all existing props unchanged, so un-migrated call sites keep compiling.

- [ ] **Step 1: Replace the view**

New signature adds `#[prop(optional)] children: Option<Children>`. New view (replacing the `<section class="panel ...">` block):

```rust
view! {
    <section class="flex flex-col gap-3">
        <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
            <h1 class="text-lg sm:text-xl font-bold text-[color:var(--brand-fg)]">
                {title.clone()}
            </h1>
            <button
                type="button"
                class="btn-secondary !p-2 rounded-full"
                title=move || if is_open() {
                    t_string!(i18n, tool_help_hide_info).to_string()
                } else {
                    t_string!(i18n, tool_help_about_tool).to_string()
                }
                aria-label=move || if is_open() {
                    t_string!(i18n, tool_help_hide_info).to_string()
                } else {
                    t_string!(i18n, tool_help_about_tool).to_string()
                }
                aria-expanded=move || if is_open() { "true" } else { "false" }
                on:click=move |_| set_is_open.update(|open| *open = !*open)
            >
                <Icon icon=i::BsInfoCircle width="1.1em" height="1.1em" />
            </button>
            {children.map(|children| view! {
                <div class="ms-auto flex flex-wrap items-center gap-3">
                    {children()}
                </div>
            })}
        </div>
        <Show when=move || is_open()>
            // existing info panel unchanged
        </Show>
    </section>
}
```

Check the repo's actual utility for icon-button padding — if `btn-secondary` already handles square icon buttons elsewhere (search for an existing icon-only button pattern) copy that instead of `!p-2`.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p ultros-app` (all call sites still compile — children optional).

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/tool_help.rs
git commit -m "refactor(ui): ToolHeader becomes a slim row with icon-only About + controls slot"
```

### Task 2: Migrate the nine call sites

**Files (all Modify):**
- `routes/analyzer.rs:3255-3264` — move `<AnalyzerWorldNavigator />` (and its wrapper row) into the slot; delete the old row.
- `routes/trends.rs:627-641` — move the world label + `TrendsWorldNavigator` into the slot; the window pills row stays where it is (it wraps too wide for the header on mobile) unless it visibly fits.
- `routes/venture_analyzer.rs:652-…` — move the `justify-end` row (Suspense status + navigator) contents into the slot.
- `routes/fc_crafting_analyzer.rs:826-…` — same pattern as venture.
- `routes/leve_analyzer.rs:770-…` — same pattern.
- `routes/recipe_analyzer.rs:1351-…` — same pattern.
- `routes/scrip_sources.rs:934-948` — move the world label + `WorldOnlyPicker` row into the slot.
- `routes/currency_exchange.rs:910-…` — move the quantity label+input row into the slot.
- `routes/vendor_resale.rs:918-924` — no slot content; the controls panel below stays. Header just shrinks.

**Interfaces:**
- Consumes: `ToolHeader` `children` slot from Task 1.

- [ ] **Step 1: Migrate each page** — pattern (analyzer.rs example):

```rust
<ToolHeader
    title=... summary=... context=... help_href=... help_body=...
>
    <AnalyzerWorldNavigator />
</ToolHeader>
// delete: <div class="flex flex-wrap items-center justify-end gap-3">...</div>
```

Preserve any comments explaining Suspense placement (analyzer.rs has one — update its wording to mention the slot).

- [ ] **Step 2: `cargo check -p ultros-app`, then `./check_ci.sh`** (fmt + clippy; check `REAL_EXIT`).

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes
git commit -m "refactor(ui): inline tool controls into the compact ToolHeader row on all nine tools"
```

### Task 3: Visual verification + PR

- [ ] **Step 1:** Build & serve locally (or use the E2E harness `./scripts/run_e2e.sh` if a build is already up); check Flip Finder, Trends, Vendor Resale, Scrip Sources at 375px and desktop. Verify: single row, ⓘ toggles the info panel, world picker works during a load.
- [ ] **Step 2:** Append a player-visible changelog entry at the TOP of `/changelog` data (per repo convention) — "Tool pages: slimmer headers; the About info now lives behind the ⓘ icon."
- [ ] **Step 3:** Push branch, open PR against `main` with before/after screenshots.
