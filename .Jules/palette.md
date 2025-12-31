# Palette's Journal

## 2024-05-22 - Redundant Alt Text Patterns
**Learning:** Found that `ItemIcon` was programmatically generating "Image for item [Name]" as alt text. This is a common pattern developers use thinking it helps, but screen readers already announce "Image", leading to "Image, Image for item Iron Ore".
**Action:** When generating alt text for item icons, use *only* the item name. The context (that it is an image) is implicit.

## 2024-05-22 - Global State in Frontend Tests
**Learning:** `ultros-app` components heavily rely on `xiv_gen_db::data()` which is a global static. This makes unit testing individual components difficult without initializing the entire game database, which is slow and complex.
**Action:** For simple logic changes in these components, rely on `cargo check` for type safety and manual verification or E2E tests where the environment is fully loaded. Avoid writing isolated unit tests for components touching global state.
