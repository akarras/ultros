CREATE TABLE plugin_rate_limit (
  key TEXT PRIMARY KEY, window_at TIMESTAMPTZ NOT NULL DEFAULT now(), count INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE plugin_grant (
  id TEXT PRIMARY KEY, owner BIGINT NOT NULL REFERENCES discord_user(id) ON DELETE CASCADE,
  label TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 64), scopes JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(), last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '90 days',
  refresh_expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '30 days',
  used_at TIMESTAMPTZ, revoked_at TIMESTAMPTZ,
  endpoint_id INTEGER REFERENCES notification_endpoint(id) ON DELETE SET NULL
);
CREATE INDEX plugin_grant_owner ON plugin_grant(owner);
CREATE TABLE plugin_pairing (
  id TEXT PRIMARY KEY, device_hash BYTEA NOT NULL UNIQUE, human_hash BYTEA NOT NULL,
  label TEXT NOT NULL, requested_scopes JSONB NOT NULL, approved_scopes JSONB,
  ip_hash BYTEA NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '5 minutes',
  state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','approved','denied','consumed')),
  owner BIGINT REFERENCES discord_user(id) ON DELETE CASCADE,
  replacement_id TEXT REFERENCES plugin_grant(id) ON DELETE CASCADE,
  next_poll_at TIMESTAMPTZ NOT NULL DEFAULT now(), poll_interval INTEGER NOT NULL DEFAULT 5
);
CREATE INDEX plugin_pairing_human ON plugin_pairing(human_hash);
CREATE UNIQUE INDEX plugin_pairing_live_code ON plugin_pairing(human_hash) WHERE state IN ('pending','approved');
CREATE TABLE plugin_token (
  id TEXT PRIMARY KEY, grant_id TEXT NOT NULL REFERENCES plugin_grant(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('access','refresh')), secret_hash BYTEA NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(), expires_at TIMESTAMPTZ NOT NULL,
  used_at TIMESTAMPTZ
);
CREATE INDEX plugin_token_grant ON plugin_token(grant_id);
CREATE TABLE plugin_approval (
  id BYTEA PRIMARY KEY, owner BIGINT NOT NULL REFERENCES discord_user(id) ON DELETE CASCADE,
  csrf BYTEA NOT NULL, nonce BYTEA,
  pairing_id TEXT REFERENCES plugin_pairing(id) ON DELETE CASCADE,
  failures INTEGER NOT NULL DEFAULT 0,
  expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '5 minutes'
);
CREATE TABLE plugin_security_event (
  id BIGSERIAL PRIMARY KEY, owner BIGINT NOT NULL REFERENCES discord_user(id) ON DELETE CASCADE,
  grant_id TEXT, kind TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE plugin_character (
  id BIGSERIAL PRIMARY KEY, owner BIGINT NOT NULL REFERENCES discord_user(id) ON DELETE CASCADE,
  fingerprint BYTEA NOT NULL, name TEXT NOT NULL, home_world INTEGER NOT NULL REFERENCES world(id),
  registered BOOLEAN NOT NULL DEFAULT false, upload_enabled BOOLEAN NOT NULL DEFAULT false,
  capture_epoch BIGINT NOT NULL DEFAULT 0, observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE(owner, fingerprint), UNIQUE(owner,id)
);
CREATE TABLE plugin_retainer (
  id BIGSERIAL PRIMARY KEY, owner BIGINT NOT NULL REFERENCES discord_user(id) ON DELETE CASCADE,
  fingerprint BYTEA NOT NULL, name TEXT NOT NULL, city INTEGER,
  observed_character BIGINT NOT NULL, registered_character BIGINT,
  revision BIGINT NOT NULL DEFAULT 0, observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE(owner,fingerprint), UNIQUE(owner,id),
  FOREIGN KEY(owner,observed_character) REFERENCES plugin_character(owner,id) ON DELETE CASCADE,
  FOREIGN KEY(owner,registered_character) REFERENCES plugin_character(owner,id) ON DELETE CASCADE
);
CREATE TABLE plugin_operation (
  owner BIGINT NOT NULL REFERENCES discord_user(id) ON DELETE CASCADE,
  kind TEXT NOT NULL, id TEXT NOT NULL, body_hash BYTEA NOT NULL, response JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY(owner,kind,id)
);
CREATE TABLE plugin_snapshot (
  id BIGSERIAL PRIMARY KEY, owner BIGINT NOT NULL, retainer_id BIGINT NOT NULL,
  revision BIGINT NOT NULL, capture_id TEXT NOT NULL, body_hash BYTEA NOT NULL,
  listings JSONB NOT NULL, observed_at TIMESTAMPTZ NOT NULL,
  received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY(owner,retainer_id) REFERENCES plugin_retainer(owner,id) ON DELETE CASCADE,
  UNIQUE(owner,retainer_id,revision)
);
CREATE TABLE plugin_delivery (
  id BIGSERIAL PRIMARY KEY, device_id TEXT NOT NULL REFERENCES plugin_grant(id) ON DELETE CASCADE,
  endpoint_id INTEGER NOT NULL REFERENCES notification_endpoint(id) ON DELETE CASCADE,
  event_id BIGINT REFERENCES alert_event(id) ON DELETE CASCADE,
  operation_id TEXT NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL,
  action JSONB NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), expires_at TIMESTAMPTZ NOT NULL,
  acknowledged_at TIMESTAMPTZ, cancelled_at TIMESTAMPTZ,
  UNIQUE(device_id,operation_id)
);
CREATE INDEX plugin_delivery_pending ON plugin_delivery(device_id,id) WHERE acknowledged_at IS NULL AND cancelled_at IS NULL;
ALTER TABLE alert_event ADD COLUMN accepted_at TIMESTAMPTZ;
ALTER TABLE alert_event ADD COLUMN pending_destinations INTEGER NOT NULL DEFAULT 0;
CREATE TABLE notification_outbox (
  id BIGSERIAL PRIMARY KEY, event_id BIGINT NOT NULL REFERENCES alert_event(id) ON DELETE CASCADE,
  endpoint_id INTEGER REFERENCES notification_endpoint(id) ON DELETE CASCADE,
  legacy_destination_id INTEGER REFERENCES alert_discord_destination(id) ON DELETE CASCADE,
  config_hash BYTEA, payload JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(), expires_at TIMESTAMPTZ NOT NULL,
  next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(), attempts INTEGER NOT NULL DEFAULT 0,
  claim TEXT, lease_until TIMESTAMPTZ, completed_at TIMESTAMPTZ, cancelled_at TIMESTAMPTZ,
  last_error TEXT, CHECK ((endpoint_id IS NOT NULL) <> (legacy_destination_id IS NOT NULL))
);
CREATE INDEX notification_outbox_pending ON notification_outbox(next_attempt_at,id) WHERE completed_at IS NULL AND cancelled_at IS NULL;
CREATE FUNCTION refresh_notification_pending() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE target BIGINT;
BEGIN
  IF TG_OP='DELETE' THEN target=OLD.event_id; ELSE target=NEW.event_id; END IF;
  UPDATE alert_event SET pending_destinations=(
    SELECT count(*) FROM (
      SELECT id FROM notification_outbox WHERE event_id=target AND completed_at IS NULL AND cancelled_at IS NULL AND expires_at>now()
      UNION ALL
      SELECT id FROM plugin_delivery WHERE event_id=target AND acknowledged_at IS NULL AND cancelled_at IS NULL AND expires_at>now()
    ) pending
  ) WHERE id=target;
  RETURN NULL;
END $$;
CREATE TRIGGER notification_outbox_counts AFTER INSERT OR UPDATE OR DELETE ON notification_outbox FOR EACH ROW EXECUTE FUNCTION refresh_notification_pending();
CREATE TRIGGER plugin_delivery_counts AFTER INSERT OR UPDATE OR DELETE ON plugin_delivery FOR EACH ROW EXECUTE FUNCTION refresh_notification_pending();
