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

        ApiResponse::new_serialized(crate::settings::load(&state).await).ok()
    }
}

mod put_settings {
    use super::*;
    use compact_str::CompactString;
    use garde::Validate;
    use serde::{Deserialize, Serialize};

    #[derive(ToSchema, Validate, Deserialize)]
    pub struct Payload {
        #[garde(
            length(chars, min = 1, max = 64),
            pattern("^[A-Za-z0-9][A-Za-z0-9._-]*$")
        )]
        integration_name: CompactString,
        #[garde(length(chars, min = 1, max = 96))]
        display_title: CompactString,
        #[garde(length(chars, min = 1, max = 64))]
        panel_name: CompactString,
    }

    #[derive(ToSchema, Serialize)]
    struct Response {}

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

        if display_title.is_empty() || panel_name.is_empty() {
            return ApiResponse::error("display title and panel name must not be blank").ok();
        }

        {
            let mut settings = state.settings.get_mut().await?;
            let extension_settings: &mut ExtensionSettingsData =
                settings.find_mut_extension_settings()?;

            extension_settings.integration_name = integration_name.into();
            extension_settings.display_title = display_title.into();
            extension_settings.panel_name = panel_name.into();

            settings.save().await?;
        }

        activity_logger
            .log(
                "settings:extensions:update",
                serde_json::json!({
                    "extension": crate::PACKAGE,
                    "changed_fields": ["integration_name", "display_title", "panel_name"],
                }),
            )
            .await;

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
