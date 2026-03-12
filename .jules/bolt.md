## 2025-03-12 - SeaORM Join Optimization
**Learning:** In SeaORM, fetching a model and mapping it manually to another table creates N+1 query patterns or 2-query manual joins which can be slow and use more memory.
**Action:** Use `.find_also_related()` on `Entity::find()` to let the database handle the `LEFT JOIN` and map the result to a `Vec<(Model, Option<RelatedModel>)>` in a single round-trip. This simplifies code and improves execution speed significantly.
