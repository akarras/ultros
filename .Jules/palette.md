## 2024-10-06 - Modal Close Button Missing Focus Style
**Learning:** Found that the modal component's close button has some hardcoded focus styling `focus:ring-2 focus:ring-[color:var(--brand-ring)]`, but some other components like `Clipboard` use `focus-visible:` for their focus states. It's better to use `focus-visible:` so mouse users don't get the focus ring when they click, but keyboard users do.
**Action:** Let's find places using `focus:ring` on buttons and change them to `focus-visible:ring`.
