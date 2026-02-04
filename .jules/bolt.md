# Bolt's Journal

This journal tracks critical performance learnings.

## 2024-05-22 - [Bolt Initialized]
**Learning:** Bolt is initialized and ready to optimize.
**Action:** Explore codebase for performance opportunities.

## 2024-05-22 - [SeaORM Join Optimization]
**Learning:** SeaORM `find_also_related` allows fetching related entities in a single query (JOIN), avoiding N+1 or 2-step fetch patterns common in the codebase.
**Action:** Look for other instances of `Entity::find()` followed by manual ID collection and second `Entity::find()` to replace with `find_also_related`.
