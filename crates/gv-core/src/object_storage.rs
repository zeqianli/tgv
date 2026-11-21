//! Reading track files from S3 through OpenDAL.
//!
//! Every request names the object it reads and what the object is for, so a failure says
//! whether the data file, its index, or its header could not be read.

use crate::error::Error;
use async_compat::{Compat, CompatExt};
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_credential_types::provider::{
    ProvideCredentials, SharedCredentialsProvider, error::CredentialsError,
};
use noodles::{csi, csi::BinningIndex, tabix};
use opendal::{FuturesAsyncReader, Operator, services};
use reqsign::{AwsCredential, AwsCredentialLoad};
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};

static AWS_SETTINGS: OnceCell<AwsSettings> = OnceCell::const_new();

struct AwsSettings {
    region: Option<String>,
    credentials: Option<Arc<CredentialCache>>,
}

impl AwsSettings {
    async fn load() -> Self {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        Self {
            region: config.region().map(|region| region.to_string()),
            credentials: config.credentials_provider().map(|provider| {
                Arc::new(CredentialCache {
                    provider,
                    credential: Mutex::new(None),
                })
            }),
        }
    }
}

struct CredentialCache {
    provider: SharedCredentialsProvider,
    credential: Mutex<Option<AwsCredential>>,
}

struct AwsConfigCredentials(Arc<CredentialCache>);

#[async_trait]
impl AwsCredentialLoad for AwsConfigCredentials {
    async fn load_credential(
        &self,
        _client: reqwest::Client,
    ) -> anyhow::Result<Option<AwsCredential>> {
        let mut cached = self.0.credential.lock().await;
        if let Some(credential) = cached.as_ref().filter(|credential| credential.is_valid()) {
            return Ok(Some(credential.clone()));
        }
        let credentials = match self.0.provider.provide_credentials().await {
            Ok(credentials) => credentials,
            Err(CredentialsError::CredentialsNotLoaded(_)) => return Ok(None),
            Err(error) => {
                let mut message = error.to_string();
                let mut previous = message.clone();
                let mut source = std::error::Error::source(&error);
                while let Some(cause) = source {
                    let cause_message = cause.to_string();
                    if cause_message != previous {
                        message.push_str(&format!(": {cause_message}"));
                    }
                    previous = cause_message;
                    source = cause.source();
                }
                return Err(anyhow::anyhow!(message));
            }
        };
        let credential = AwsCredential {
            access_key_id: credentials.access_key_id().to_owned(),
            secret_access_key: credentials.secret_access_key().to_owned(),
            session_token: credentials.session_token().map(str::to_owned),
            expires_in: credentials.expiry().map(chrono::DateTime::from),
        };
        *cached = Some(credential.clone());
        Ok(Some(credential))
    }
}

/// An object in an S3 bucket, parsed from an `s3://<bucket>/<key>` URL.
#[derive(Debug, Clone)]
pub struct S3Object {
    pub url: String,
    pub bucket: String,
    pub key: String,
}

impl S3Object {
    pub fn parse(url: &str) -> Result<Self, Error> {
        let invalid = |reason| Error::InvalidObjectUrl {
            url: url.to_owned(),
            reason,
        };
        let rest = url
            .strip_prefix("s3://")
            .ok_or_else(|| invalid("only s3:// URLs are supported"))?;
        let (bucket, key) = rest
            .split_once('/')
            .ok_or_else(|| invalid("the URL has no object key"))?;
        if bucket.is_empty() {
            return Err(invalid("the URL has no bucket"));
        }
        if key.is_empty() {
            return Err(invalid("the URL has no object key"));
        }
        Ok(Self {
            url: url.to_owned(),
            bucket: bucket.to_owned(),
            key: key.to_owned(),
        })
    }

    /// The object whose URL is this one's with `suffix` appended, such as an index.
    pub fn with_suffix(&self, suffix: &str) -> Self {
        Self {
            url: format!("{}{suffix}", self.url),
            bucket: self.bucket.clone(),
            key: format!("{}{suffix}", self.key),
        }
    }

    pub async fn operator(&self) -> Result<Operator, Error> {
        let settings = AWS_SETTINGS.get_or_init(AwsSettings::load).await;
        let mut builder = services::S3::default().bucket(&self.bucket);
        if let Some(region) = &settings.region {
            builder = builder.region(region);
        }
        if let Some(credentials) = &settings.credentials {
            builder = builder.customized_credential_load(Box::new(AwsConfigCredentials(
                Arc::clone(credentials),
            )));
        }
        Operator::new(builder)
            .map(|builder| builder.finish())
            .map_err(|source| Error::ObjectStorage {
                operation: "configure S3 access for",
                url: self.url.clone(),
                source: Box::new(source),
            })
    }

    /// Opens a seekable reader over the whole object. `operation` describes the read for
    /// logs and errors, such as "read the BAM index".
    pub async fn open_read(
        &self,
        operator: &Operator,
        operation: &'static str,
    ) -> Result<Compat<FuturesAsyncReader>, Error> {
        self.log_request(operation);
        let error = |source| Error::ObjectStorage {
            operation,
            url: self.url.clone(),
            source: Box::new(source),
        };
        Ok(operator
            .reader(&self.key)
            .await
            .map_err(error)?
            .into_futures_async_read(..)
            .await
            .map_err(error)?
            .compat())
    }

    /// Reads the whole object into memory.
    pub async fn read_all(
        &self,
        operator: &Operator,
        operation: &'static str,
    ) -> Result<Vec<u8>, Error> {
        self.log_request(operation);
        operator
            .read(&self.key)
            .await
            .map(|buffer| buffer.to_vec())
            .map_err(|source| Error::ObjectStorage {
                operation,
                url: self.url.clone(),
                source: Box::new(source),
            })
    }

    /// Whether the object exists. Errors other than a missing object, such as denied access,
    /// are reported rather than read as a missing object.
    pub async fn exists(
        &self,
        operator: &Operator,
        operation: &'static str,
    ) -> Result<bool, Error> {
        self.log_request(operation);
        operator
            .exists(&self.key)
            .await
            .map_err(|source| Error::ObjectStorage {
                operation,
                url: self.url.clone(),
                source: Box::new(source),
            })
    }

    /// Wraps an error from reading or parsing the object's contents.
    pub fn read_error(&self, operation: &'static str) -> impl Fn(std::io::Error) -> Error {
        let url = self.url.clone();
        move |source| Error::ObjectStorageRead {
            operation,
            url: url.clone(),
            source,
        }
    }

    fn log_request(&self, operation: &str) {
        log::info!(
            "Object storage request: object_url={} bucket={} key={} context={operation}",
            self.url,
            self.bucket,
            self.key,
        );
    }
}

/// Reads the tabix index next to a bgzipped object, or its CSI index when there is no tabix
/// index. `None` when neither exists.
pub async fn read_tabix_or_csi_index(
    data: &S3Object,
    operator: &Operator,
) -> Result<Option<Box<dyn BinningIndex + Send + Sync>>, Error> {
    const READ: &str = "read the tabix index";
    let tbi = data.with_suffix(".tbi");
    if tbi.exists(operator, "check for the tabix index").await? {
        let index = tabix::r#async::io::Reader::new(tbi.open_read(operator, READ).await?)
            .read_index()
            .await
            .map_err(tbi.read_error(READ))?;
        return Ok(Some(Box::new(index)));
    }
    read_csi_index(data, operator).await
}

/// Reads the CSI index next to an object. `None` when it doesn't exist.
pub async fn read_csi_index(
    data: &S3Object,
    operator: &Operator,
) -> Result<Option<Box<dyn BinningIndex + Send + Sync>>, Error> {
    const READ: &str = "read the CSI index";
    let csi = data.with_suffix(".csi");
    if !csi.exists(operator, "check for the CSI index").await? {
        return Ok(None);
    }
    let index = csi::r#async::io::Reader::new(csi.open_read(operator, READ).await?)
        .read_index()
        .await
        .map_err(csi.read_error(READ))?;
    Ok(Some(Box::new(index)))
}

/// The contig names in an index's header, which region queries must use.
pub fn index_contigs(index: &dyn BinningIndex) -> Vec<String> {
    index
        .header()
        .map(|header| {
            header
                .reference_sequence_names()
                .iter()
                .map(|name| name.to_string())
                .collect()
        })
        .unwrap_or_default()
}
