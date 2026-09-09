## 2024-09-09 - Insecure Direct Object Reference in Discord DM Endpoint Creation
**Vulnerability:** A user could create a Discord DM notification endpoint for any arbitrary Discord user by passing a different `user_id` in the `EndpointMethod::DiscordDm` payload.
**Learning:** `user_id` inside Discord DM endpoint configurations was implicitly trusted from user payloads rather than enforced against the authenticated user's ID at validation time.
**Prevention:** In `validate_endpoint_method`, pass the `owner_id` (the authenticated user's ID) and explicitly verify that the payload's `user_id` matches the `owner_id` to prevent arbitrary notification routing.
