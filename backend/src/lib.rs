use shared::{
    State,
    extensions::{Extension, ExtensionRouteBuilder},
};

mod api;
mod mcp;
mod openapi;
mod proxy;
mod settings;
mod tools;

pub const PACKAGE: &str = "dev.caloptreyx.calmcp";

#[derive(Default)]
pub struct ExtensionStruct;

#[async_trait::async_trait]
impl Extension for ExtensionStruct {
    async fn settings_deserializer(
        &self,
        _state: State,
    ) -> shared::extensions::settings::ExtensionSettingsDeserializer {
        std::sync::Arc::new(settings::ExtensionSettingsDataDeserializer)
    }

    async fn initialize_router(
        &mut self,
        state: State,
        builder: ExtensionRouteBuilder,
    ) -> ExtensionRouteBuilder {
        builder
            .add_client_api_router(|routes| {
                routes
                    .nest(&format!("/extensions/{PACKAGE}"), mcp::router(&state))
                    .nest(
                        &format!("/extensions/{PACKAGE}"),
                        api::client_router(&state),
                    )
            })
            .add_admin_api_router(|routes| {
                routes.nest(&format!("/extensions/{PACKAGE}"), api::admin_router(&state))
            })
    }
}
