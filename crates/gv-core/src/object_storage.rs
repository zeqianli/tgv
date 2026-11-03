//! Reading track files from S3 through OpenDAL.
//!
//! Every request names the object it reads and what the object is for, so a failure says
//! whether the data file, its index, or its header could not be read.

use crate::error::TGVError;
use async_compat::{Compat, CompatExt};
use noodles::{csi, csi::BinningIndex, tabix};
use opendal::{FuturesAsyncReader, Operator, services};

/// An object in an S3 bucket, parsed from an `s3://<bucket>/<key>` URL.
#[derive(Debug, Clone)]
pub struct S3Object {
    pub url: String,
    pub bucket: String,
    pub key: String,
}

impl S3Object {
    pub fn parse(url: &str) -> Result<Self, TGVError> {
        let invalid = |reason| TGVError::InvalidObjectUrl {
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

    /// Builds an operator for the object's bucket. Credentials and the region come from the
    /// environment.
    pub fn operator(&self) -> Result<Operator, TGVError> {
        Operator::new(services::S3::default().bucket(&self.bucket))
            .map(|builder| builder.finish())
            .map_err(|source| TGVError::ObjectStorage {
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
    ) -> Result<Compat<FuturesAsyncReader>, TGVError> {
        self.log_request(operation);
        let error = |source| TGVError::ObjectStorage {
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
    ) -> Result<Vec<u8>, TGVError> {
        self.log_request(operation);
        operator
            .read(&self.key)
            .await
            .map(|buffer| buffer.to_vec())
            .map_err(|source| TGVError::ObjectStorage {
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
    ) -> Result<bool, TGVError> {
        self.log_request(operation);
        operator
            .exists(&self.key)
            .await
            .map_err(|source| TGVError::ObjectStorage {
                operation,
                url: self.url.clone(),
                source: Box::new(source),
            })
    }

    /// Wraps an error from reading or parsing the object's contents.
    pub fn read_error(&self, operation: &'static str) -> impl Fn(std::io::Error) -> TGVError {
        let url = self.url.clone();
        move |source| TGVError::ObjectStorageRead {
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
) -> Result<Option<Box<dyn BinningIndex + Send + Sync>>, TGVError> {
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
) -> Result<Option<Box<dyn BinningIndex + Send + Sync>>, TGVError> {
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
