-- OAuth 2.1 for MCP web connectors. Every grant is backed by a panel API key that carries the
-- user's permissions; deleting that key (here, on the API keys page or with the user) ends the grant.
-- Secrets, codes and tokens are stored as sha256 digests only.

CREATE TABLE dev_caloptreyx_calmcp_oauth_clients (
    uuid uuid NOT NULL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    redirect_uris TEXT[] NOT NULL,
    auth_method VARCHAR(31) NOT NULL,
    secret_digest VARCHAR(64),
    created TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE dev_caloptreyx_calmcp_oauth_grants (
    uuid uuid NOT NULL PRIMARY KEY,
    client_uuid uuid NOT NULL REFERENCES dev_caloptreyx_calmcp_oauth_clients(uuid) ON DELETE CASCADE,
    user_uuid uuid NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    api_key_uuid uuid NOT NULL UNIQUE REFERENCES user_api_keys(uuid) ON DELETE CASCADE,
    scope VARCHAR(15) NOT NULL,
    resource TEXT NOT NULL,
    created TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX dev_caloptreyx_calmcp_oauth_grants_user_uuid_idx
    ON dev_caloptreyx_calmcp_oauth_grants (user_uuid);
CREATE INDEX dev_caloptreyx_calmcp_oauth_grants_client_uuid_idx
    ON dev_caloptreyx_calmcp_oauth_grants (client_uuid);

CREATE TABLE dev_caloptreyx_calmcp_oauth_codes (
    digest VARCHAR(64) NOT NULL PRIMARY KEY,
    client_uuid uuid NOT NULL REFERENCES dev_caloptreyx_calmcp_oauth_clients(uuid) ON DELETE CASCADE,
    user_uuid uuid NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    redirect_uri_explicit BOOLEAN NOT NULL,
    code_challenge VARCHAR(128) NOT NULL,
    scope VARCHAR(15) NOT NULL,
    expires TIMESTAMPTZ NOT NULL,
    used BOOLEAN NOT NULL DEFAULT false,
    grant_uuid uuid REFERENCES dev_caloptreyx_calmcp_oauth_grants(uuid) ON DELETE SET NULL
);

CREATE INDEX dev_caloptreyx_calmcp_oauth_codes_expires_idx
    ON dev_caloptreyx_calmcp_oauth_codes (expires);

CREATE TABLE dev_caloptreyx_calmcp_oauth_tokens (
    digest VARCHAR(64) NOT NULL PRIMARY KEY,
    grant_uuid uuid NOT NULL REFERENCES dev_caloptreyx_calmcp_oauth_grants(uuid) ON DELETE CASCADE,
    kind VARCHAR(7) NOT NULL,
    expires TIMESTAMPTZ NOT NULL,
    used BOOLEAN NOT NULL DEFAULT false
);

CREATE INDEX dev_caloptreyx_calmcp_oauth_tokens_grant_uuid_idx
    ON dev_caloptreyx_calmcp_oauth_tokens (grant_uuid);
CREATE INDEX dev_caloptreyx_calmcp_oauth_tokens_expires_idx
    ON dev_caloptreyx_calmcp_oauth_tokens (expires);
