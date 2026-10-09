//! Base-quality encodings of alignment files.
//!
//! BAM stores each base quality as a raw byte that should equal the Phred score. Files
//! converted from Phred+64 FASTQ (Illumina 1.3–1.7) without re-encoding store the Phred score
//! plus 31 instead. The alignment tables hold the Phred score, so loading subtracts the offset
//! of the file's encoding, and displays add it back to show the stored values.

use clap::ValueEnum;
use noodles::sam::alignment::RecordBuf;

/// How an alignment file stores base qualities.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum QualityEncoding {
    /// Stored bytes are Phred scores, as the BAM specification requires.
    #[default]
    Phred33,
    /// Stored bytes are Phred scores plus 31, from Phred+64 FASTQ.
    Phred64,
}

impl QualityEncoding {
    /// The difference between Phred+64 and Phred+33 text, which Phred+64 BAM bytes carry.
    const PHRED64_OFFSET: u8 = 31;

    /// The highest Phred+33 score, which SAM text encodes as `~`.
    const PHRED33_MAX: u8 = 93;

    /// The stored byte that marks missing qualities.
    const MISSING: u8 = 0xFF;

    /// The difference between a stored byte and the Phred score.
    pub fn offset(self) -> u8 {
        match self {
            Self::Phred33 => 0,
            Self::Phred64 => Self::PHRED64_OFFSET,
        }
    }

    /// Converts a stored byte to the Phred score.
    pub fn phred(self, stored: u8) -> u8 {
        stored.saturating_sub(self.offset())
    }

    /// Converts a Phred score back to the stored byte.
    pub fn stored(self, phred: u8) -> u8 {
        phred.saturating_add(self.offset())
    }

    /// Infers the encoding from the stored qualities of a record batch.
    ///
    /// Returns `None` when no record has qualities. Nearly all files are Phred+33, so a batch
    /// is Phred+64 only when a byte exceeds the Phred+33 maximum. Phred+64 files whose bytes
    /// all stay within that maximum read as Phred+33 and need an explicit setting.
    pub fn detect(records: &[RecordBuf]) -> Option<Self> {
        let mut scores = records
            .iter()
            .flat_map(|record| record.quality_scores().as_ref().iter().copied())
            .peekable();
        scores.peek()?;
        let beyond_phred33 =
            scores.any(|score| score > Self::PHRED33_MAX && score != Self::MISSING);
        Some(if beyond_phred33 {
            Self::Phred64
        } else {
            Self::Phred33
        })
    }
}

/// Chooses the quality encoding of alignment files.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum QualityEncodingSetting {
    /// Assume Phred+33, unless the first loaded records with qualities hold a byte above Q93.
    #[default]
    #[value(name = "auto")]
    Auto,
    /// Stored bytes are Phred scores (Phred+33).
    #[value(name = "33")]
    Phred33,
    /// Stored bytes are Phred scores plus 31 (Phred+64).
    #[value(name = "64")]
    Phred64,
}

impl QualityEncodingSetting {
    /// Returns the encoding for a record batch, inferring it on the first batch with qualities
    /// when the setting is automatic.
    ///
    /// Batches without qualities leave an automatic setting unresolved, since the encoding
    /// does not affect them.
    pub fn resolve(&mut self, records: &[RecordBuf]) -> QualityEncoding {
        match *self {
            Self::Phred33 => QualityEncoding::Phred33,
            Self::Phred64 => QualityEncoding::Phred64,
            Self::Auto => match QualityEncoding::detect(records) {
                Some(encoding) => {
                    log::info!("Detected the base-quality encoding: {encoding:?}");
                    *self = encoding.into();
                    encoding
                }
                None => QualityEncoding::default(),
            },
        }
    }
}

impl From<QualityEncoding> for QualityEncodingSetting {
    fn from(encoding: QualityEncoding) -> Self {
        match encoding {
            QualityEncoding::Phred33 => Self::Phred33,
            QualityEncoding::Phred64 => Self::Phred64,
        }
    }
}
