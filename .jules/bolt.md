## 2024-05-14 - Eliminated N+1 roundtrip in active listing queries
**Learning:** `SeaORM` provides `.find_also_related()` which allows fetching an entity along with its related entity using a JOIN. This is much faster and more concise than querying an entity first, manually collecting related IDs, fetching them in a second query, and zip-mapping them in memory.
**Action:** When I see a manual N+1 join in SeaORM code (querying one entity and then building an ID list to query its relationships), I will replace it with `.find_also_related()`.
