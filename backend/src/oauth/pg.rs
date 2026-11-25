//! The panel's PostgreSQL implementation of [`Store`]. Queries are static strings with bound
//! parameters; tables are created by `migrations/`.

use super::{
    Scope,
    store::{
        AuthMethod, Client, Code, CodeOutcome, Grant, GrantSummary, NewGrant, RefreshOutcome,
        Store, Token,
    },
};
use chrono::{DateTime, NaiveDateTime, Utc};
use compact_str::CompactString;
use shared::{
    State,
    models::{
        CreatableModel, DeletableModel,
        user_api_key::{CreateUserApiKeyOptions, UserApiKey},
    },
};
use sqlx::{Row, postgres::PgRow};
use std::time::Duration;
use uuid::Uuid;

pub struct PgStore {
    state: State,
}

impl PgStore {
    pub fn new(state: &State) -> Self {
        Self {
            state: state.clone(),
        }
    }

    fn pool(&self) -> &sqlx::PgPool {
        self.state.database.write()
    }
}

fn scope(row: &PgRow) -> Result<Scope, anyhow::Error> {
    let value: String = row.try_get("scope")?;
    Scope::parse(&value).ok_or_else(|| anyhow::anyhow!("unknown OAuth scope {value} in the database"))
}

fn grant(row: &PgRow) -> Result<Grant, anyhow::Error> {
    Ok(Grant {
        uuid: row.try_get("uuid")?,
        client_uuid: row.try_get("client_uuid")?,
        user_uuid: row.try_get("user_uuid")?,
        api_key_uuid: row.try_get("api_key_uuid")?,
        scope: scope(row)?,
        resource: row.try_get("resource")?,
    })
}

/// Every permission of one kind the panel and its extensions currently define.
fn all_permissions(list: &std::collections::HashSet<String>) -> Vec<CompactString> {
    let mut permissions: Vec<CompactString> = list.iter().map(CompactString::from).collect();
    permissions.sort_unstable();
    permissions
}

/// The panel API key name: unique per user, at most 31 characters.
fn api_key_name(client_name: &str, grant_uuid: Uuid) -> CompactString {
    let suffix = &grant_uuid.simple().to_string()[..6];
    let name: String = client_name.chars().take(31 - 4 - 7).collect();
    let name = name.trim();

    if name.is_empty() {
        format!("MCP {suffix}").into()
    } else {
        format!("MCP {name} {suffix}").into()
    }
}

/// The options for a grant's api key, built through serde rather than a struct literal: panel
/// main (after 1.2.11) added a required `unrestricted_admin`, which older panels lack and ignore
/// when deserializing. `false` keeps the key out of the actions reserved for an unrestricted
/// administrator, as every api key was before that field existed.
pub(super) fn api_key_options(
    user_uuid: Uuid,
    name: CompactString,
    user_permissions: Vec<CompactString>,
    admin_permissions: Vec<CompactString>,
    server_permissions: Vec<CompactString>,
) -> Result<CreateUserApiKeyOptions, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "user_uuid": user_uuid,
        "name": name,
        "allowed_ips": [],
        "enabled": true,
        "user_permissions": user_permissions,
        "admin_permissions": admin_permissions,
        "server_permissions": server_permissions,
        "unrestricted_admin": false,
        "expires": null,
    }))
}

#[async_trait::async_trait]
impl Store for PgStore {
    async fn insert_client(&self, client: &Client) -> Result<(), anyhow::Error> {
        sqlx::query(
            "INSERT INTO dev_caloptreyx_calmcp_oauth_clients
                (uuid, name, redirect_uris, auth_method, secret_digest)
            VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(client.uuid)
        .bind(&client.name)
        .bind(&client.redirect_uris)
        .bind(client.auth_method.as_str())
        .bind(&client.secret_digest)
        .execute(self.pool())
        .await?;

        Ok(())
    }

    async fn client(&self, uuid: Uuid) -> Result<Option<Client>, anyhow::Error> {
        let Some(row) = sqlx::query(
            "SELECT uuid, name, redirect_uris, auth_method, secret_digest
            FROM dev_caloptreyx_calmcp_oauth_clients WHERE uuid = $1",
        )
        .bind(uuid)
        .fetch_optional(self.pool())
        .await?
        else {
            return Ok(None);
        };

        let auth_method: String = row.try_get("auth_method")?;
        Ok(Some(Client {
            uuid: row.try_get("uuid")?,
            name: row.try_get("name")?,
            redirect_uris: row.try_get("redirect_uris")?,
            auth_method: AuthMethod::parse(&auth_method)
                .ok_or_else(|| anyhow::anyhow!("unknown client auth method {auth_method}"))?,
            secret_digest: row.try_get("secret_digest")?,
        }))
    }

    async fn count_grants(&self, user_uuid: Uuid) -> Result<i64, anyhow::Error> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM dev_caloptreyx_calmcp_oauth_grants WHERE user_uuid = $1",
        )
        .bind(user_uuid)
        .fetch_one(self.pool())
        .await?)
    }

    async fn insert_code(&self, code: &Code) -> Result<(), anyhow::Error> {
        sqlx::query(
            "INSERT INTO dev_caloptreyx_calmcp_oauth_codes
                (digest, client_uuid, user_uuid, redirect_uri, redirect_uri_explicit,
                code_challenge, scope, expires)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&code.digest)
        .bind(code.client_uuid)
        .bind(code.user_uuid)
        .bind(&code.redirect_uri)
        .bind(code.redirect_uri_explicit)
        .bind(&code.code_challenge)
        .bind(code.scope.as_str())
        .bind(code.expires)
        .execute(self.pool())
        .await?;

        Ok(())
    }

    async fn take_code(
        &self,
        digest: &str,
        now: DateTime<Utc>,
    ) -> Result<CodeOutcome, anyhow::Error> {
        let taken = sqlx::query(
            "UPDATE dev_caloptreyx_calmcp_oauth_codes SET used = true
            WHERE digest = $1 AND NOT used AND expires > $2
            RETURNING client_uuid, user_uuid, redirect_uri, redirect_uri_explicit,
                code_challenge, scope, expires",
        )
        .bind(digest)
        .bind(now)
        .fetch_optional(self.pool())
        .await?;

        if let Some(row) = taken {
            return Ok(CodeOutcome::Fresh(Code {
                digest: digest.to_string(),
                client_uuid: row.try_get("client_uuid")?,
                user_uuid: row.try_get("user_uuid")?,
                redirect_uri: row.try_get("redirect_uri")?,
                redirect_uri_explicit: row.try_get("redirect_uri_explicit")?,
                code_challenge: row.try_get("code_challenge")?,
                scope: scope(&row)?,
                expires: row.try_get("expires")?,
            }));
        }

        let spent = sqlx::query(
            "SELECT grant_uuid FROM dev_caloptreyx_calmcp_oauth_codes WHERE digest = $1 AND used",
        )
        .bind(digest)
        .fetch_optional(self.pool())
        .await?;

        Ok(match spent {
            Some(row) => CodeOutcome::Replayed(row.try_get("grant_uuid")?),
            None => CodeOutcome::Invalid,
        })
    }

    async fn create_grant(&self, new: &NewGrant) -> Result<Grant, anyhow::Error> {
        // No credential management: see `CREDENTIAL_PERMISSIONS`.
        let user_permissions: Vec<CompactString> = super::grant_user_permissions(
            shared::permissions::get_user_permissions().list(),
        )
        .into_iter()
        .map(CompactString::from)
        .collect();
        let server_permissions =
            all_permissions(shared::permissions::get_server_permissions().list());
        // Client-only grants get no admin permissions, so the panel itself refuses /api/admin.
        let admin_permissions = if new.scope.access().client_only {
            Vec::new()
        } else {
            all_permissions(shared::permissions::get_admin_permissions().list())
        };

        let mut transaction = self.pool().begin().await?;

        let options = api_key_options(
            new.user_uuid,
            api_key_name(&new.client_name, new.uuid),
            user_permissions,
            admin_permissions,
            server_permissions,
        )?;
        let (_, api_key) =
            UserApiKey::create_with_transaction(&self.state, options, &mut transaction).await?;

        sqlx::query(
            "INSERT INTO dev_caloptreyx_calmcp_oauth_grants
                (uuid, client_uuid, user_uuid, api_key_uuid, scope, resource)
            VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(new.uuid)
        .bind(new.client_uuid)
        .bind(new.user_uuid)
        .bind(api_key.uuid)
        .bind(new.scope.as_str())
        .bind(&new.resource)
        .execute(&mut *transaction)
        .await?;

        sqlx::query("UPDATE dev_caloptreyx_calmcp_oauth_codes SET grant_uuid = $1 WHERE digest = $2")
            .bind(new.uuid)
            .bind(&new.code_digest)
            .execute(&mut *transaction)
            .await?;

        insert_tokens(&mut transaction, new.uuid, &new.access, &new.refresh).await?;

        transaction.commit().await?;

        Ok(Grant {
            uuid: new.uuid,
            client_uuid: new.client_uuid,
            user_uuid: new.user_uuid,
            api_key_uuid: api_key.uuid,
            scope: new.scope,
            resource: new.resource.clone(),
        })
    }

    async fn rotate_refresh(
        &self,
        digest: &str,
        client_uuid: Uuid,
        requested: Option<Scope>,
        now: DateTime<Utc>,
        access: &Token,
        refresh: &Token,
    ) -> Result<RefreshOutcome, anyhow::Error> {
        let mut transaction = self.pool().begin().await?;

        let row = sqlx::query(
            "SELECT t.used, t.expires, g.uuid, g.client_uuid, g.user_uuid, g.api_key_uuid,
                g.scope, g.resource
            FROM dev_caloptreyx_calmcp_oauth_tokens t
            JOIN dev_caloptreyx_calmcp_oauth_grants g ON g.uuid = t.grant_uuid
            WHERE t.digest = $1 AND t.kind = 'refresh'
            FOR UPDATE OF t",
        )
        .bind(digest)
        .fetch_optional(&mut *transaction)
        .await?;

        let Some(row) = row else {
            return Ok(RefreshOutcome::Invalid);
        };
        let grant = grant(&row)?;
        let used: bool = row.try_get("used")?;
        let expires: DateTime<Utc> = row.try_get("expires")?;

        if grant.client_uuid != client_uuid {
            return Ok(RefreshOutcome::Invalid);
        }
        if used {
            return Ok(RefreshOutcome::Replayed(grant.uuid));
        }
        if expires <= now {
            return Ok(RefreshOutcome::Invalid);
        }
        if requested.is_some_and(|requested| requested > grant.scope) {
            return Ok(RefreshOutcome::ScopeExceeded);
        }

        sqlx::query("UPDATE dev_caloptreyx_calmcp_oauth_tokens SET used = true WHERE digest = $1")
            .bind(digest)
            .execute(&mut *transaction)
            .await?;
        insert_tokens(&mut transaction, grant.uuid, access, refresh).await?;

        // Keep each grant's token rows bounded however often it refreshes: the newest access
        // tokens (older ones may still be in flight for a moment) and the newest spent refresh
        // tokens, which are enough to catch a replay of a recently rotated one.
        sqlx::query(
            "DELETE FROM dev_caloptreyx_calmcp_oauth_tokens
            WHERE grant_uuid = $1 AND kind = 'access' AND (expires <= $2 OR digest NOT IN (
                SELECT digest FROM dev_caloptreyx_calmcp_oauth_tokens
                WHERE grant_uuid = $1 AND kind = 'access'
                ORDER BY expires DESC LIMIT $3
            ))",
        )
        .bind(grant.uuid)
        .bind(now)
        .bind(super::KEPT_ACCESS_TOKENS)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "DELETE FROM dev_caloptreyx_calmcp_oauth_tokens
            WHERE grant_uuid = $1 AND kind = 'refresh' AND used AND digest NOT IN (
                SELECT digest FROM dev_caloptreyx_calmcp_oauth_tokens
                WHERE grant_uuid = $1 AND kind = 'refresh' AND used
                ORDER BY expires DESC LIMIT $2
            )",
        )
        .bind(grant.uuid)
        .bind(super::KEPT_SPENT_REFRESH_TOKENS)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        Ok(RefreshOutcome::Rotated(grant))
    }

    async fn access(
        &self,
        digest: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<Grant>, anyhow::Error> {
        sqlx::query(
            "SELECT g.uuid, g.client_uuid, g.user_uuid, g.api_key_uuid, g.scope, g.resource
            FROM dev_caloptreyx_calmcp_oauth_tokens t
            JOIN dev_caloptreyx_calmcp_oauth_grants g ON g.uuid = t.grant_uuid
            WHERE t.digest = $1 AND t.kind = 'access' AND t.expires > $2",
        )
        .bind(digest)
        .bind(now)
        .fetch_optional(self.pool())
        .await?
        .as_ref()
        .map(grant)
        .transpose()
    }

    async fn token_grant(&self, digest: &str) -> Result<Option<Grant>, anyhow::Error> {
        sqlx::query(
            "SELECT g.uuid, g.client_uuid, g.user_uuid, g.api_key_uuid, g.scope, g.resource
            FROM dev_caloptreyx_calmcp_oauth_tokens t
            JOIN dev_caloptreyx_calmcp_oauth_grants g ON g.uuid = t.grant_uuid
            WHERE t.digest = $1",
        )
        .bind(digest)
        .fetch_optional(self.pool())
        .await?
        .as_ref()
        .map(grant)
        .transpose()
    }

    async fn revoke(&self, grant_uuid: Uuid) -> Result<(), anyhow::Error> {
        let Some(row) = sqlx::query(
            "SELECT user_uuid, api_key_uuid FROM dev_caloptreyx_calmcp_oauth_grants WHERE uuid = $1",
        )
        .bind(grant_uuid)
        .fetch_optional(self.pool())
        .await?
        else {
            return Ok(());
        };

        // Deleting the key through the panel also clears its caches; the grant, its tokens and
        // the link from its code go with it through the foreign keys.
        if let Some(api_key) = UserApiKey::by_user_uuid_uuid(
            &self.state.database,
            row.try_get("user_uuid")?,
            row.try_get("api_key_uuid")?,
        )
        .await?
        {
            api_key.delete(&self.state, ()).await?;
        }

        sqlx::query("DELETE FROM dev_caloptreyx_calmcp_oauth_grants WHERE uuid = $1")
            .bind(grant_uuid)
            .execute(self.pool())
            .await?;

        Ok(())
    }

    async fn list_grants(&self, user_uuid: Uuid) -> Result<Vec<GrantSummary>, anyhow::Error> {
        let rows = sqlx::query(
            "SELECT g.uuid, c.name, c.redirect_uris, g.scope, g.created, k.last_used
            FROM dev_caloptreyx_calmcp_oauth_grants g
            JOIN dev_caloptreyx_calmcp_oauth_clients c ON c.uuid = g.client_uuid
            JOIN user_api_keys k ON k.uuid = g.api_key_uuid
            WHERE g.user_uuid = $1
            ORDER BY g.created DESC",
        )
        .bind(user_uuid)
        .fetch_all(self.pool())
        .await?;

        rows.iter()
            .map(|row| -> Result<GrantSummary, anyhow::Error> {
                let last_used: Option<NaiveDateTime> = row.try_get("last_used")?;
                Ok(GrantSummary {
                    uuid: row.try_get("uuid")?,
                    client_name: row.try_get("name")?,
                    redirect_uris: row.try_get("redirect_uris")?,
                    scope: scope(row)?,
                    created: row.try_get("created")?,
                    last_used: last_used.map(|last_used| last_used.and_utc()),
                })
            })
            .collect()
    }

    async fn user_grant(
        &self,
        user_uuid: Uuid,
        grant_uuid: Uuid,
    ) -> Result<Option<Grant>, anyhow::Error> {
        sqlx::query(
            "SELECT g.uuid, g.client_uuid, g.user_uuid, g.api_key_uuid, g.scope, g.resource
            FROM dev_caloptreyx_calmcp_oauth_grants g
            WHERE g.uuid = $1 AND g.user_uuid = $2",
        )
        .bind(grant_uuid)
        .bind(user_uuid)
        .fetch_optional(self.pool())
        .await?
        .as_ref()
        .map(grant)
        .transpose()
    }
}

async fn insert_tokens(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    grant_uuid: Uuid,
    access: &Token,
    refresh: &Token,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "INSERT INTO dev_caloptreyx_calmcp_oauth_tokens (digest, grant_uuid, kind, expires)
        VALUES ($1, $3, 'access', $4), ($2, $3, 'refresh', $5)",
    )
    .bind(&access.digest)
    .bind(&refresh.digest)
    .bind(grant_uuid)
    .bind(access.expires)
    .bind(refresh.expires)
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// One cleanup pass, then an hour's pause before the next.
pub async fn prune(state: &State) -> Result<(), anyhow::Error> {
    let pool = state.database.write();

    sqlx::query("DELETE FROM dev_caloptreyx_calmcp_oauth_codes WHERE expires < now() - interval '1 hour'")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM dev_caloptreyx_calmcp_oauth_tokens WHERE expires < now()")
        .execute(pool)
        .await?;

    // Grants nobody can refresh any more. The grace hour keeps a grant whose code is being
    // exchanged right now.
    let stale: Vec<Uuid> = sqlx::query_scalar(
        "SELECT g.uuid FROM dev_caloptreyx_calmcp_oauth_grants g
        WHERE g.created < now() - interval '1 hour'
            AND NOT EXISTS (
                SELECT 1 FROM dev_caloptreyx_calmcp_oauth_tokens t
                WHERE t.grant_uuid = g.uuid AND t.kind = 'refresh' AND NOT t.used
            )",
    )
    .fetch_all(pool)
    .await?;
    let store = PgStore::new(state);
    for grant_uuid in stale {
        store.revoke(grant_uuid).await?;
    }

    sqlx::query(
        "DELETE FROM dev_caloptreyx_calmcp_oauth_clients c
        WHERE c.created < now() - interval '30 days'
            AND NOT EXISTS (
                SELECT 1 FROM dev_caloptreyx_calmcp_oauth_grants g WHERE g.client_uuid = c.uuid
            )",
    )
    .execute(pool)
    .await?;

    tokio::time::sleep(Duration::from_secs(60 * 60)).await;
    Ok(())
}
