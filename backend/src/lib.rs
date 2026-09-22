use shared::{
    State,
    extensions::{Extension, ExtensionRouteBuilder},
};

mod mcp;
mod openapi;
mod proxy;
mod tools;

pub const PACKAGE: &str = "dev.caloptreyx.calmcp";

#[derive(Default)]
pub struct ExtensionStruct;

#[async_trait::async_trait]
impl Extension for ExtensionStruct {
    async fn initialize_router(
        &mut self,
        state: State,
        builder: ExtensionRouteBuilder,
    ) -> ExtensionRouteBuilder {
        builder.add_client_api_router(|routes| {
            routes.nest(&format!("/extensions/{PACKAGE}"), mcp::router(&state))
        })
    }
}
