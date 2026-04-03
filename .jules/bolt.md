## 2024-04-03 - Database Upsert Optimization
**Learning:** SeaORM supports `ON CONFLICT` clauses which map directly to SQL `INSERT ... ON CONFLICT DO UPDATE`. The previous code was manually handling updates and inserts by cloning the model and executing multiple queries: trying update, then insert, then update again.
**Action:** Replace multi-query manual update/insert loops with single query `.on_conflict()` inserts to reduce database round-trips from up to 3 down to exactly 1.
