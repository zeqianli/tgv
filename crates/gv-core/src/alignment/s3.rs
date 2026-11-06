//! S3 access for remote alignment files.
//!
//! opendal signs S3 requests with credentials from reqsign, which reads environment variables,
//! static keys in the shared config files, web identity tokens, and instance metadata, but not
//! SSO sessions or `credential_process`. For those, this module asks the AWS CLI with
//! `aws configure export-credentials`, so every profile the CLI can use works here too,
//! including one signed in with `aws sso login`. Asking the CLI keeps the AWS SDK and its
//! dependency tree out of the build.

use crate::error::TGVError;

use anyhow::{Context, anyhow, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use opendal::{Operator, services};
use reqsign::{AwsConfig, AwsCredential, AwsCredentialLoad, AwsDefaultLoader};
use serde::Deserialize;
use std::{io, sync::Arc, time::Duration};
use tokio::{process::Command, sync::Mutex};

const DEFAULT_S3_ENDPOINT: &str = "https://s3.amazonaws.com";

/// Environment variables that point S3 at another endpoint, such as S3-compatible storage.
const ENDPOINT_ENV_VARS: [&str; 2] = ["AWS_ENDPOINT_URL_S3", "AWS_ENDPOINT_URL"];

/// How long the AWS CLI may take to print credentials. SSO refreshes make a network call.
const CLI_TIMEOUT: Duration = Duration::from_secs(30);

/// Builds S3 operators that share one credential loader.
pub struct S3Access {
    loader: CredentialLoader,
}

impl S3Access {
    /// `profile` names an AWS profile. `None` follows the AWS CLI: `AWS_PROFILE`, or the
    /// `default` profile.
    pub fn new(profile: Option<&str>) -> Self {
        Self {
            loader: CredentialLoader::new(profile),
        }
    }

    /// Builds an operator for `bucket`.
    pub async fn operator(&self, bucket: &str) -> Result<Operator, TGVError> {
        let mut builder = services::S3::default()
            .bucket(bucket)
            .customized_credential_load(Box::new(self.loader.clone()));

        // A profile's region is often not the bucket's, and requests to the wrong region fail,
        // so ask AWS S3 where the bucket is. Other endpoints keep the configured region, which
        // opendal reads from the environment and the profile.
        let custom_endpoint = ENDPOINT_ENV_VARS
            .iter()
            .any(|name| std::env::var_os(name).is_some());
        if !custom_endpoint
            && let Some(region) = services::S3::detect_region(DEFAULT_S3_ENDPOINT, bucket).await
        {
            builder = builder.region(&region);
        }

        Ok(Operator::new(builder)?.finish())
    }
}

/// Loads credentials the way the AWS CLI resolves them.
///
/// With an explicit profile, the AWS CLI is the only source, because the CLI's `--profile`
/// takes precedence over credentials in the environment. Without one, reqsign's sources go
/// first so that plain keys don't start a process. Instance metadata comes last, and only
/// when the AWS CLI isn't installed, since the CLI checks it too.
///
/// opendal asks for credentials before every request, so the loader keeps the last
/// credentials until they are about to expire. Clones share the cache.
#[derive(Clone)]
struct CredentialLoader {
    profile: Option<String>,
    local: Arc<AwsDefaultLoader>,
    with_instance_metadata: Arc<AwsDefaultLoader>,
    cached: Arc<Mutex<Option<AwsCredential>>>,
}

impl CredentialLoader {
    fn new(profile: Option<&str>) -> Self {
        let client = reqwest::Client::new();
        let config = AwsConfig::default().from_profile().from_env();
        Self {
            profile: profile.map(str::to_owned),
            local: Arc::new(
                AwsDefaultLoader::new(client.clone(), config.clone()).with_disable_ec2_metadata(),
            ),
            with_instance_metadata: Arc::new(AwsDefaultLoader::new(client, config)),
            cached: Arc::default(),
        }
    }

    async fn load_uncached(&self) -> anyhow::Result<Option<AwsCredential>> {
        if let Some(profile) = &self.profile {
            return match export_credentials(Some(profile)).await? {
                Some(credential) => Ok(Some(credential)),
                None => bail!(
                    "reading profile {profile} needs AWS CLI v2, and the `aws` command isn't \
                     installed"
                ),
            };
        }

        if let Some(credential) = self.local.load().await? {
            return Ok(Some(credential));
        }
        match export_credentials(None).await? {
            Some(credential) => Ok(Some(credential)),
            None => self.with_instance_metadata.load().await,
        }
    }
}

#[async_trait]
impl AwsCredentialLoad for CredentialLoader {
    async fn load_credential(
        &self,
        _client: reqwest::Client,
    ) -> anyhow::Result<Option<AwsCredential>> {
        // Holding the lock while loading keeps concurrent requests from each starting the CLI.
        let mut cached = self.cached.lock().await;
        if let Some(credential) = cached.as_ref().filter(|credential| credential.is_valid()) {
            return Ok(Some(credential.clone()));
        }

        let credential = self.load_uncached().await?;
        cached.clone_from(&credential);
        Ok(credential)
    }
}

/// The `process` format of `aws configure export-credentials`.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExportedCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    /// RFC 3339, such as `2026-11-06T18:00:00+00:00`.
    expiration: Option<String>,
}

/// Runs `aws configure export-credentials`. Returns `None` when the AWS CLI isn't installed.
async fn export_credentials(profile: Option<&str>) -> anyhow::Result<Option<AwsCredential>> {
    let mut command = Command::new("aws");
    command.args(["configure", "export-credentials", "--format", "process"]);
    if let Some(profile) = profile {
        command.args(["--profile", profile]);
    }
    // The viewer owns the terminal, so the CLI must not read from it.
    command
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);

    let output = match tokio::time::timeout(CLI_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Ok(Err(err)) => {
            return Err(err).context("failed to run `aws configure export-credentials`");
        }
        Err(_) => bail!(
            "`aws configure export-credentials` didn't finish in {} seconds",
            CLI_TIMEOUT.as_secs()
        ),
    };

    if !output.status.success() {
        // The CLI's message says what to do, such as running `aws sso login`.
        return Err(anyhow!(
            "`aws configure export-credentials` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let exported: ExportedCredentials = serde_json::from_slice(&output.stdout)
        .context("`aws configure export-credentials` printed unexpected output")?;
    let expires_in = exported
        .expiration
        .map(|expiration| {
            DateTime::parse_from_rfc3339(&expiration)
                .map(|expiration| expiration.with_timezone(&Utc))
                .with_context(|| format!("the AWS CLI printed an invalid expiration: {expiration}"))
        })
        .transpose()?;
    Ok(Some(AwsCredential {
        access_key_id: exported.access_key_id,
        secret_access_key: exported.secret_access_key,
        session_token: exported.session_token,
        expires_in,
    }))
}
