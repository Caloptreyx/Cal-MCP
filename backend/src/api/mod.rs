use axum::http::StatusCode;
use shared::{
    ApiError, GetState,
    models::{admin_activity::GetAdminActivityLogger, user::GetPermissionManager},
    response::{ApiResponse, ApiResponseResult},
};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::settings::PublicSettings;

mod get_branding {
    use super::*;

    #[utoipa::path(get, path = "/branding", responses(
        (status = OK, body = inline(PublicSettings)),
    ))]
    pub async fn route(state: GetState) -> ApiResponseResult {
        ApiResponse::new_serialized(crate::settings::load(&state).await.public()).ok()
    }
}

mod get_settings {
    use super::*;

    #[utoipa::path(get, path = "/settings", responses(
        (status = OK, body = inline(PublicSettings)),
    ))]
    pub async fn route(state: GetState, permissions: GetPermissionManager) -> ApiResponseResult {
        permissions.has_admin_permission("extensions.manage")?;

        ApiResponse::new_serialized(crate::settings::load_result(&state).await?.public()).ok()
    }
}

mod put_settings {
    use super::*;
    use crate::{access::Access, settings::ExtensionSettingsData};
    use compact_str::CompactString;
    use garde::Validate;
    use serde::{Deserialize, Serialize};

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Payload {
        #[garde(custom(validate_integration_name))]
        integration_name: CompactString,
        #[garde(custom(validate_display_title))]
        display_title: CompactString,
        #[garde(custom(validate_panel_name))]
        panel_name: CompactString,
        /// Make every MCP connection read-only.
        #[garde(skip)]
        #[serde(default)]
        read_only: bool,
        /// Block /api/admin for every MCP connection.
        #[garde(skip)]
        #[serde(default)]
        client_only: bool,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {}

    fn validate_integration_name(value: &CompactString, _context: &()) -> Result<(), garde::Error> {
        if crate::settings::normalize_integration_name(value).is_some() {
            return Ok(());
        }

        Err(garde::Error::new(
            "must start with a letter or number and only contain letters, numbers, dots, dashes and underscores (max 64 characters)",
        ))
    }

    fn validate_display_title(value: &CompactString, _context: &()) -> Result<(), garde::Error> {
        if crate::settings::normalize_display_text(value, crate::settings::MAX_DISPLAY_TITLE)
            .is_some()
        {
            return Ok(());
        }

        Err(garde::Error::new(
            "must be 1-96 characters and must not contain control characters",
        ))
    }

    fn validate_panel_name(value: &CompactString, _context: &()) -> Result<(), garde::Error> {
        if crate::settings::normalize_display_text(value, crate::settings::MAX_PANEL_NAME).is_some()
        {
            return Ok(());
        }

        Err(garde::Error::new(
            "must be 1-64 characters and must not contain control characters",
        ))
    }

    #[utoipa::path(put, path = "/settings", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
    ), request_body = inline(Payload))]
    pub async fn route(
        state: GetState,
        permissions: GetPermissionManager,
        activity_logger: GetAdminActivityLogger,
        shared::Payload(data): shared::Payload<Payload>,
    ) -> ApiResponseResult {
        permissions.has_admin_permission("extensions.manage")?;

        if let Err(errors) = shared::utils::validate_data(&data) {
            return ApiResponse::new_serialized(ApiError::new_strings_value(errors))
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        {
            let mut settings = state.settings.get_mut().await?;
            let extension_settings: &mut ExtensionSettingsData =
                settings.get_mut_extension_settings(crate::PACKAGE)?;
            let previous = extension_settings.public();

            extension_settings.integration_name = data.integration_name.trim().into();
            extension_settings.display_title = data.display_title.trim().into();
            extension_settings.panel_name = data.panel_name.trim().into();
            extension_settings.restrictions = Access {
                read_only: data.read_only,
                client_only: data.client_only,
            };
            let current = extension_settings.public();

            let changed_fields: Vec<&str> = [
                (
                    "integration_name",
                    previous.integration_name != current.integration_name,
                ),
                (
                    "display_title",
                    previous.display_title != current.display_title,
                ),
                ("panel_name", previous.panel_name != current.panel_name),
                ("read_only", previous.read_only != current.read_only),
                ("client_only", previous.client_only != current.client_only),
            ]
            .into_iter()
            .filter_map(|(name, changed)| changed.then_some(name))
            .collect();

            settings.save().await?;

            if !changed_fields.is_empty() {
                activity_logger
                    .log(
                        "settings:extensions:update",
                        serde_json::json!({
                            "extension": crate::PACKAGE,
                            "changed_fields": changed_fields,
                            "old": previous,
                            "new": current,
                        }),
                    )
                    .await;
            }
        }

        ApiResponse::new_serialized(Response {}).ok()
    }
}

mod get_key_restrictions {
    use super::*;
    use crate::access::Access;
    use serde::Serialize;
    use shared::models::user::GetUser;

    #[derive(ToSchema, Serialize)]
    pub struct KeyAccess {
        api_key: uuid::Uuid,
        #[serde(flatten)]
        access: Access,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {
        /// Restrictions an administrator applied to every connection.
        panel: Access,
        /// Restrictions you chose for your own API keys. Keys that are not
        /// listed have none.
        keys: Vec<KeyAccess>,
    }

    #[utoipa::path(get, path = "/restrictions", responses(
        (status = OK, body = inline(Response)),
    ))]
    pub async fn route(state: GetState, user: GetUser) -> ApiResponseResult {
        let settings = crate::settings::load_result(&state).await?;

        ApiResponse::new_serialized(Response {
            panel: settings.restrictions,
            keys: settings
                .key_restrictions
                .iter()
                .filter(|(_, entry)| entry.user == user.uuid)
                .map(|(api_key, entry)| KeyAccess {
                    api_key: *api_key,
                    access: entry.access,
                })
                .collect(),
        })
        .ok()
    }
}

mod put_key_restrictions {
    use super::*;
    use crate::{
        access::Access,
        settings::{ExtensionSettingsData, KeyRestriction, MAX_KEY_RESTRICTIONS_PER_USER},
    };
    use axum::extract::Path;
    use serde::Serialize;
    use shared::models::{
        user::{AuthMethod, GetAuthMethod, GetUser},
        user_activity::GetUserActivityLogger,
        user_api_key::UserApiKey,
    };

    #[derive(ToSchema, Serialize)]
    struct Response {}

    #[utoipa::path(put, path = "/restrictions/{api_key}", responses(
        (status = OK, body = inline(Response)),
        (status = FORBIDDEN, body = ApiError),
        (status = NOT_FOUND, body = ApiError),
        (status = EXPECTATION_FAILED, body = ApiError),
    ), params(
        (
            "api_key" = uuid::Uuid,
            description = "The API key ID",
            example = "123e4567-e89b-12d3-a456-426614174000",
        ),
    ), request_body = inline(Access))]
    pub async fn route(
        state: GetState,
        user: GetUser,
        auth: GetAuthMethod,
        activity_logger: GetUserActivityLogger,
        Path(api_key): Path<uuid::Uuid>,
        shared::Payload(data): shared::Payload<Access>,
    ) -> ApiResponseResult {
        // An API key must not be able to lift its own restrictions.
        if !matches!(auth.0.as_ref(), AuthMethod::Session(_)) {
            return ApiResponse::error("MCP key restrictions can only be changed while signed in")
                .with_status(StatusCode::FORBIDDEN)
                .ok();
        }

        if UserApiKey::by_user_uuid_uuid(&state.database, user.uuid, api_key)
            .await?
            .is_none()
        {
            return ApiResponse::error("api key not found")
                .with_status(StatusCode::NOT_FOUND)
                .ok();
        }

        // Forget the user's choices for keys that no longer exist.
        let mut stale = Vec::new();
        {
            let settings = crate::settings::load_result(&state).await?;
            for (other, entry) in &settings.key_restrictions {
                if entry.user == user.uuid
                    && *other != api_key
                    && UserApiKey::by_user_uuid_uuid(&state.database, user.uuid, *other)
                        .await?
                        .is_none()
                {
                    stale.push(*other);
                }
            }
        }

        let previous = {
            let mut settings = state.settings.get_mut().await?;
            let extension_settings: &mut ExtensionSettingsData =
                settings.get_mut_extension_settings(crate::PACKAGE)?;

            for other in &stale {
                extension_settings.key_restrictions.remove(other);
            }

            let previous = extension_settings
                .key_restrictions
                .get(&api_key)
                .map(|entry| entry.access)
                .unwrap_or_default();

            if data.is_none() {
                extension_settings.key_restrictions.remove(&api_key);
            } else {
                let owned = extension_settings
                    .key_restrictions
                    .iter()
                    .filter(|(key, entry)| entry.user == user.uuid && **key != api_key)
                    .count();
                if owned >= MAX_KEY_RESTRICTIONS_PER_USER {
                    return ApiResponse::error(format!(
                        "you can restrict at most {MAX_KEY_RESTRICTIONS_PER_USER} API keys"
                    ))
                    .with_status(StatusCode::EXPECTATION_FAILED)
                    .ok();
                }

                extension_settings.key_restrictions.insert(
                    api_key,
                    KeyRestriction {
                        user: user.uuid,
                        access: data,
                    },
                );
            }

            settings.save().await?;
            previous
        };

        if previous != data {
            activity_logger
                .log(
                    "account:mcp.restrictions",
                    serde_json::json!({
                        "api_key": api_key,
                        "old": previous,
                        "new": data,
                    }),
                )
                .await;
        }

        ApiResponse::new_serialized(Response {}).ok()
    }
}

pub fn client_router(state: &shared::State) -> OpenApiRouter<shared::State> {
    OpenApiRouter::new()
        .routes(routes!(get_branding::route))
        .routes(routes!(get_key_restrictions::route))
        .routes(routes!(put_key_restrictions::route))
        .with_state(state.clone())
}

pub fn admin_router(state: &shared::State) -> OpenApiRouter<shared::State> {
    OpenApiRouter::new()
        .routes(routes!(get_settings::route, put_settings::route))
        .with_state(state.clone())
}
