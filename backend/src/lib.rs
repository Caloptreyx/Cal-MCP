use shared::{
    State,
    extensions::{Extension, ExtensionRouteBuilder, ExtensionUpdateInfo},
};

mod api;
mod mcp;
mod openapi;
mod proxy;
mod settings;
mod tools;

pub const PACKAGE: &str = "dev.caloptreyx.calmcp";

const LATEST_RELEASE: &str = "https://api.github.com/repos/Caloptreyx/Cal-MCP/releases/latest";

/// The subset of GitHub's release payload the updates page needs.
#[derive(serde::Serialize, serde::Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: String,
}

fn version_from_tag(tag: &str) -> Result<semver::Version, semver::Error> {
    semver::Version::parse(tag.trim_start_matches('v'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_from_tag_accepts_prefixed_and_bare_tags() {
        assert_eq!(version_from_tag("v1.0.0").unwrap().to_string(), "1.0.0");
        assert_eq!(version_from_tag("1.2.3").unwrap().to_string(), "1.2.3");
        assert!(version_from_tag("release-1").is_err());
    }
}

#[derive(Default)]
pub struct ExtensionStruct;

#[async_trait::async_trait]
impl Extension for ExtensionStruct {
    async fn check_for_updates(
        &self,
        state: State,
        current_version: &semver::Version,
    ) -> Result<Option<ExtensionUpdateInfo>, anyhow::Error> {
        // GitHub allows 60 unauthenticated requests per hour, and the Panel
        // rechecks on every start and every 12 hours.
        let release: Release = state
            .cache
            .cached("dev.caloptreyx.calmcp::latest-release", 60 * 60, || async {
                let response = state.client.get(LATEST_RELEASE).send().await?;
                Ok::<Release, anyhow::Error>(response.error_for_status()?.json::<Release>().await?)
            })
            .await?;

        let latest = version_from_tag(&release.tag_name)?;

        Ok((latest > *current_version).then(|| ExtensionUpdateInfo {
            version: latest,
            changes: release
                .body
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(compact_str::CompactString::from)
                .collect(),
        }))
    }

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
