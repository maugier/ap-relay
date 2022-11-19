use crate::{
    data::{ActorCache, State},
    error::Error,
    extractors::{AdminConfig, XApiToken},
    middleware::MyVerify,
    requests::Requests,
};
use activitystreams::{
    iri,
    iri_string::{
        resolve::FixedBaseResolver,
        types::{IriAbsoluteString, IriFragmentStr, IriRelativeStr, IriString},
    },
};
use config::Environment;
use http_signature_normalization_actix::prelude::{VerifyDigest, VerifySignature};
use sha2::{Digest, Sha256};
use std::{net::IpAddr, path::PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct ParsedConfig {
    hostname: String,
    addr: IpAddr,
    port: u16,
    debug: bool,
    restricted_mode: bool,
    validate_signatures: bool,
    https: bool,
    publish_blocks: bool,
    sled_path: PathBuf,
    source_repo: IriString,
    opentelemetry_url: Option<IriString>,
    telegram_token: Option<String>,
    telegram_admin_handle: Option<String>,
    api_token: Option<String>,
}

#[derive(Clone)]
pub struct Config {
    hostname: String,
    addr: IpAddr,
    port: u16,
    debug: bool,
    restricted_mode: bool,
    validate_signatures: bool,
    publish_blocks: bool,
    base_uri: IriAbsoluteString,
    sled_path: PathBuf,
    source_repo: IriString,
    opentelemetry_url: Option<IriString>,
    telegram_token: Option<String>,
    telegram_admin_handle: Option<String>,
    api_token: Option<String>,
}

#[derive(Debug)]
pub enum UrlKind {
    Activity,
    Actor,
    Followers,
    Following,
    Inbox,
    Index,
    MainKey,
    Media(Uuid),
    NodeInfo,
    Outbox,
}

#[derive(Debug)]
pub enum AdminUrlKind {
    Allow,
    Disallow,
    Block,
    Unblock,
    Allowed,
    Blocked,
    Connected,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("hostname", &self.hostname)
            .field("addr", &self.addr)
            .field("port", &self.port)
            .field("debug", &self.debug)
            .field("restricted_mode", &self.restricted_mode)
            .field("validate_signatures", &self.validate_signatures)
            .field("publish_blocks", &self.publish_blocks)
            .field("base_uri", &self.base_uri.to_string())
            .field("sled_path", &self.sled_path)
            .field("source_repo", &self.source_repo.to_string())
            .field(
                "opentelemetry_url",
                &self.opentelemetry_url.as_ref().map(|url| url.to_string()),
            )
            .field("telegram_token", &"[redacted]")
            .field("telegram_admin_handle", &self.telegram_admin_handle)
            .field("api_token", &"[redacted]")
            .finish()
    }
}

impl Config {
    pub(crate) fn build() -> Result<Self, Error> {
        let config = config::Config::builder()
            .set_default("hostname", "localhost:8080")?
            .set_default("addr", "127.0.0.1")?
            .set_default("port", 8080u64)?
            .set_default("debug", true)?
            .set_default("restricted_mode", false)?
            .set_default("validate_signatures", false)?
            .set_default("https", false)?
            .set_default("publish_blocks", false)?
            .set_default("sled_path", "./sled/db-0-34")?
            .set_default("source_repo", "https://git.asonix.dog/asonix/relay")?
            .set_default("opentelemetry_url", None as Option<&str>)?
            .set_default("telegram_token", None as Option<&str>)?
            .set_default("telegram_admin_handle", None as Option<&str>)?
            .set_default("api_token", None as Option<&str>)?
            .add_source(Environment::default())
            .build()?;

        let config: ParsedConfig = config.try_deserialize()?;

        let scheme = if config.https { "https" } else { "http" };
        let base_uri = iri!(format!("{}://{}", scheme, config.hostname)).into_absolute();

        Ok(Config {
            hostname: config.hostname,
            addr: config.addr,
            port: config.port,
            debug: config.debug,
            restricted_mode: config.restricted_mode,
            validate_signatures: config.validate_signatures,
            publish_blocks: config.publish_blocks,
            base_uri,
            sled_path: config.sled_path,
            source_repo: config.source_repo,
            opentelemetry_url: config.opentelemetry_url,
            telegram_token: config.telegram_token,
            telegram_admin_handle: config.telegram_admin_handle,
            api_token: config.api_token,
        })
    }

    pub(crate) fn sled_path(&self) -> &PathBuf {
        &self.sled_path
    }

    pub(crate) fn validate_signatures(&self) -> bool {
        self.validate_signatures
    }

    pub(crate) fn digest_middleware(&self) -> VerifyDigest<Sha256> {
        if self.validate_signatures {
            VerifyDigest::new(Sha256::new())
        } else {
            VerifyDigest::new(Sha256::new()).optional()
        }
    }

    pub(crate) fn signature_middleware(
        &self,
        requests: Requests,
        actors: ActorCache,
        state: State,
    ) -> VerifySignature<MyVerify> {
        if self.validate_signatures {
            VerifySignature::new(MyVerify(requests, actors, state), Default::default())
        } else {
            VerifySignature::new(MyVerify(requests, actors, state), Default::default()).optional()
        }
    }

    pub(crate) fn x_api_token(&self) -> Option<XApiToken> {
        self.api_token.clone().map(XApiToken::new)
    }

    pub(crate) fn admin_config(&self) -> Option<actix_web::web::Data<AdminConfig>> {
        if let Some(api_token) = &self.api_token {
            match AdminConfig::build(api_token) {
                Ok(conf) => Some(actix_web::web::Data::new(conf)),
                Err(e) => {
                    tracing::error!("Error creating admin config: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }

    pub(crate) fn bind_address(&self) -> (IpAddr, u16) {
        (self.addr, self.port)
    }

    pub(crate) fn debug(&self) -> bool {
        self.debug
    }

    pub(crate) fn publish_blocks(&self) -> bool {
        self.publish_blocks
    }

    pub(crate) fn restricted_mode(&self) -> bool {
        self.restricted_mode
    }

    pub(crate) fn hostname(&self) -> &str {
        &self.hostname
    }

    pub(crate) fn generate_resource(&self) -> String {
        format!("relay@{}", self.hostname)
    }

    pub(crate) fn software_name() -> &'static str {
        "AodeRelay"
    }

    pub(crate) fn software_version() -> String {
        if let Some(git) = Self::git_version() {
            return format!("v{}-{}", Self::version(), git);
        }

        format!("v{}", Self::version())
    }

    fn git_version() -> Option<String> {
        let branch = Self::git_branch()?;
        let hash = Self::git_hash()?;

        Some(format!("{}-{}", branch, hash))
    }

    fn name() -> &'static str {
        env!("PKG_NAME")
    }

    fn version() -> &'static str {
        env!("PKG_VERSION")
    }

    fn git_branch() -> Option<&'static str> {
        option_env!("GIT_BRANCH")
    }

    fn git_hash() -> Option<&'static str> {
        option_env!("GIT_HASH")
    }

    pub(crate) fn user_agent(&self) -> String {
        format!(
            "{} ({}/{}; +{})",
            Self::software_name(),
            Self::name(),
            Self::software_version(),
            self.generate_url(UrlKind::Index),
        )
    }

    pub(crate) fn source_code(&self) -> &IriString {
        &self.source_repo
    }

    pub(crate) fn opentelemetry_url(&self) -> Option<&IriString> {
        self.opentelemetry_url.as_ref()
    }

    pub(crate) fn telegram_info(&self) -> Option<(&str, &str)> {
        self.telegram_token.as_deref().and_then(|token| {
            let handle = self.telegram_admin_handle.as_deref()?;
            Some((token, handle))
        })
    }

    pub(crate) fn generate_url(&self, kind: UrlKind) -> IriString {
        self.do_generate_url(kind).expect("Generated valid IRI")
    }

    #[tracing::instrument(level = "debug", skip_all, fields(base_uri = tracing::field::debug(&self.base_uri), kind = tracing::field::debug(&kind)))]
    fn do_generate_url(&self, kind: UrlKind) -> Result<IriString, Error> {
        let iri = match kind {
            UrlKind::Activity => FixedBaseResolver::new(self.base_uri.as_ref()).try_resolve(
                IriRelativeStr::new(&format!("activity/{}", Uuid::new_v4()))?.as_ref(),
            )?,
            UrlKind::Actor => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("actor")?.as_ref())?,
            UrlKind::Followers => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("followers")?.as_ref())?,
            UrlKind::Following => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("following")?.as_ref())?,
            UrlKind::Inbox => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("inbox")?.as_ref())?,
            UrlKind::Index => self.base_uri.clone().into(),
            UrlKind::MainKey => {
                let actor = IriRelativeStr::new("actor")?;
                let fragment = IriFragmentStr::new("main-key")?;

                let mut resolved =
                    FixedBaseResolver::new(self.base_uri.as_ref()).try_resolve(actor.as_ref())?;

                resolved.set_fragment(Some(fragment));
                resolved
            }
            UrlKind::Media(uuid) => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new(&format!("media/{}", uuid))?.as_ref())?,
            UrlKind::NodeInfo => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("nodeinfo/2.0.json")?.as_ref())?,
            UrlKind::Outbox => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("outbox")?.as_ref())?,
        };

        Ok(iri)
    }

    pub(crate) fn generate_admin_url(&self, kind: AdminUrlKind) -> IriString {
        self.do_generate_admin_url(kind)
            .expect("Generated valid IRI")
    }

    fn do_generate_admin_url(&self, kind: AdminUrlKind) -> Result<IriString, Error> {
        let iri = match kind {
            AdminUrlKind::Allow => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("api/v1/admin/allow")?.as_ref())?,
            AdminUrlKind::Disallow => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("api/v1/admin/disallow")?.as_ref())?,
            AdminUrlKind::Block => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("api/v1/admin/block")?.as_ref())?,
            AdminUrlKind::Unblock => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("api/v1/admin/unblock")?.as_ref())?,
            AdminUrlKind::Allowed => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("api/v1/admin/allowed")?.as_ref())?,
            AdminUrlKind::Blocked => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("api/v1/admin/blocked")?.as_ref())?,
            AdminUrlKind::Connected => FixedBaseResolver::new(self.base_uri.as_ref())
                .try_resolve(IriRelativeStr::new("api/v1/admin/connected")?.as_ref())?,
        };

        Ok(iri)
    }
}
