use axum::http::StatusCode;
use shared::{
    ApiError, GetState,
    models::{admin_activity::GetAdminActivityLogger, user::GetPermissionManager},
    response::{ApiResponse, ApiResponseResult},
};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::settings::ExtensionSettingsData;

mod get_branding {
    use super::*;

    #[utoipa::path(get, path = "/branding", responses(
        (status = OK, body = inline(ExtensionSettingsData)),
    ))]
    pub async fn route(state: GetState) -> ApiResponseResult {
        ApiResponse::new_serialized(crate::settings::load(&state).await).ok()
    }
}

mod get_settings {
    use super::*;

    #[utoipa::path(get, path = "/settings", responses(
        (status = OK, body = inline(ExtensionSettingsData)),
    ))]
    pub async fn route(state: GetState, permissions: GetPermissionManager) -> ApiResponseResult {
        permissions.has_admin_permission("extensions.manage")?;

        ApiResponse::new_serialized(crate::settings::load_result(&state).await?).ok()
    }
}

mod put_settings {
    use super::*;
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

        let integration_name = data.integration_name.trim();
        let display_title = data.display_title.trim();
        let panel_name = data.panel_name.trim();

        {
            let mut settings = state.settings.get_mut().await?;
            let extension_settings: &mut ExtensionSettingsData =
                settings.get_mut_extension_settings(crate::PACKAGE)?;
            let previous = extension_settings.clone();

            extension_settings.integration_name = integration_name.into();
            extension_settings.display_title = display_title.into();
            extension_settings.panel_name = panel_name.into();

            let changed_fields: Vec<&str> = [
                (
                    "integration_name",
                    previous.integration_name != extension_settings.integration_name,
                ),
                (
                    "display_title",
                    previous.display_title != extension_settings.display_title,
                ),
                (
                    "panel_name",
                    previous.panel_name != extension_settings.panel_name,
                ),
            ]
            .into_iter()
            .filter_map(|(name, changed)| changed.then_some(name))
            .collect();

            let old_values = serde_json::json!({
                "integration_name": previous.integration_name.clone(),
                "display_title": previous.display_title.clone(),
                "panel_name": previous.panel_name.clone(),
            });
            let new_values = serde_json::json!({
                "integration_name": extension_settings.integration_name.clone(),
                "display_title": extension_settings.display_title.clone(),
                "panel_name": extension_settings.panel_name.clone(),
            });

            settings.save().await?;

            if !changed_fields.is_empty() {
                activity_logger
                    .log(
                        "settings:extensions:update",
                        serde_json::json!({
                            "extension": crate::PACKAGE,
                            "changed_fields": changed_fields,
                            "old": old_values,
                            "new": new_values,
                        }),
                    )
                    .await;
            }
        }

        ApiResponse::new_serialized(Response {}).ok()
    }
}

pub fn client_router(state: &shared::State) -> OpenApiRouter<shared::State> {
    OpenApiRouter::new()
        .routes(routes!(get_branding::route))
        .with_state(state.clone())
}

pub fn admin_router(state: &shared::State) -> OpenApiRouter<shared::State> {
    OpenApiRouter::new()
        .routes(routes!(get_settings::route, put_settings::route))
        .with_state(state.clone())
}
