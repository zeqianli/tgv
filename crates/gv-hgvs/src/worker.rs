//! A worker thread that owns the loaded HGVS data and serves requests over channels.
//!
//! ferro-hgvs is synchronous, and loading its data takes seconds, so [`HgvsService::spawn`]
//! loads it on a dedicated thread. Requests queue until loading finishes, and
//! [`HgvsHandle::status`] reports progress. [`Engine`] does the work and can also be used
//! directly from synchronous code.

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use ferro_hgvs::data::mapping::CoordinateMapper;
use ferro_hgvs::data::projection::Projector;
use ferro_hgvs::hgvs::interval::GenomeInterval;
use ferro_hgvs::hgvs::location::{CdsPos, GenomePos};
use ferro_hgvs::project::VariantProjector;
use ferro_hgvs::reference::multi_fasta::MultiFastaProvider;
use ferro_hgvs::reference::transcript::GenomeBuild;
use ferro_hgvs::vcf::{VcfRecord, vcf_to_genomic_hgvs};
use ferro_hgvs::{HgvsVariant, VariantProjection, parse_hgvs};
use gv_core::message::{Message, Movement, Zoom};
use lru::LruCache;
use tokio::sync::{mpsc, oneshot, watch};

use crate::assembly::{HgvsBuild, HgvsDataDir};
use crate::error::HgvsError;
use crate::query::{
    CdsBase, CodingPosition, ContigAccession, GeneSymbol, HgvsLocation, HgvsQuery, HgvsTarget,
    TranscriptAccession,
};

/// A search whose gene symbol, if any, has been replaced by the gene's preferred transcript.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedQuery {
    pub target: ResolvedTarget,
    pub location: HgvsLocation,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ResolvedTarget {
    Transcript(TranscriptAccession),
    Genomic(ContigAccession),
}

impl HgvsQuery {
    /// The gene symbol that needs a transcript before the query can resolve.
    pub fn gene(&self) -> Option<&GeneSymbol> {
        match &self.target {
            HgvsTarget::Gene(gene) => Some(gene),
            _ => None,
        }
    }

    /// Replaces a gene target with its transcript. tgv takes the transcript from the gene
    /// track, which is RefSeq Select for hg38 and hg19, because ferro-hgvs has no reliable
    /// preferred transcript per gene. `transcript` is ignored for other targets.
    pub fn resolve(
        self,
        transcript: Option<TranscriptAccession>,
    ) -> Result<ResolvedQuery, HgvsError> {
        let target = match self.target {
            HgvsTarget::Gene(gene) => ResolvedTarget::Transcript(
                transcript.ok_or(HgvsError::UnknownGene { gene: gene.0 })?,
            ),
            HgvsTarget::Transcript(transcript) => ResolvedTarget::Transcript(transcript),
            HgvsTarget::Genomic(contig) => ResolvedTarget::Genomic(contig),
        };
        Ok(ResolvedQuery {
            target,
            location: self.location,
        })
    }
}

/// A 1-based, inclusive genomic interval on a tgv contig.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GenomicSpan {
    /// The tgv contig name, such as `chr12`.
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

impl GenomicSpan {
    /// Messages that center the view on the span, the same as a region search. A single base
    /// keeps the current zoom.
    pub fn messages(&self) -> Vec<Message> {
        let center = self.start + (self.end - self.start) / 2;
        let mut messages = vec![Message::Move(Movement::ContigNamePosition(
            self.contig.clone(),
            center,
        ))];
        if self.end > self.start {
            messages.push(
                Zoom::Fit {
                    bases: self.end - self.start + 1,
                }
                .into(),
            );
        }
        messages
    }
}

/// A variant allele, as in a VCF record. `pos` is the 1-based position of `reference`.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct VariantKey {
    pub contig: String,
    pub pos: u64,
    pub reference: String,
    pub alternate: String,
}

/// Which transcripts to describe a variant on.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum AnnotationScope {
    /// Only these transcripts, such as the RefSeq Select transcripts at the variant.
    Transcripts(Vec<TranscriptAccession>),
    /// ferro-hgvs's curated set of overlapping transcripts, MANE Select first.
    AllOverlapping,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TranscriptRegion {
    Exonic,
    Intronic,
    Utr,
}

/// A variant described on one transcript.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TranscriptHgvs {
    pub transcript: String,
    pub gene: Option<String>,
    /// The `c.` or `n.` description.
    pub transcript_hgvs: Option<String>,
    pub protein: Option<String>,
    pub region: TranscriptRegion,
}

/// A variant described in HGVS.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VariantHgvs {
    /// The normalized `g.` description.
    pub genomic: String,
    pub transcripts: Vec<TranscriptHgvs>,
}

/// The loaded HGVS data and the operations on it.
pub struct Engine {
    build: HgvsBuild,
    mapper: CoordinateMapper,
    projector: VariantProjector<Arc<MultiFastaProvider>>,
    annotations: Mutex<LruCache<(VariantKey, AnnotationScope), VariantHgvs>>,
}

impl Engine {
    const ANNOTATION_CACHE: NonZeroUsize = NonZeroUsize::new(1024).unwrap();

    /// Loads prepared data. This takes seconds, and ferro-hgvs reports on standard error.
    pub fn load(data_dir: &HgvsDataDir) -> Result<Self, HgvsError> {
        data_dir.check_prepared()?;
        let provider = MultiFastaProvider::from_manifest(data_dir.manifest())?;
        let cdot = provider
            .cdot_mapper()
            .cloned()
            .ok_or_else(|| HgvsError::Unavailable {
                reason: format!(
                    "{} has no transcript models. Run `tgv download {}` again.",
                    data_dir.path.display(),
                    data_dir.reference
                ),
            })?;
        // The projector takes ownership of its own mapper and doesn't expose it, so position
        // lookups keep a second copy.
        let mapper = CoordinateMapper::new(cdot.clone());
        let projector = VariantProjector::new(Projector::new(cdot), Arc::new(provider));
        Ok(Self {
            build: data_dir.build,
            mapper,
            projector,
            annotations: Mutex::new(LruCache::new(Self::ANNOTATION_CACHE)),
        })
    }

    /// The genomic interval of a search.
    pub fn resolve(&self, query: &ResolvedQuery) -> Result<GenomicSpan, HgvsError> {
        match (&query.target, &query.location) {
            (ResolvedTarget::Genomic(contig), HgvsLocation::Genomic { start, end }) => {
                Ok(GenomicSpan {
                    contig: self.contig_name(&contig.0)?,
                    start: *start,
                    end: *end,
                })
            }
            (ResolvedTarget::Genomic(contig), HgvsLocation::Expression(expression)) => {
                self.expression_span(&contig.0, expression)
            }
            (ResolvedTarget::Transcript(transcript), HgvsLocation::Coding { start, end }) => {
                let positions = std::iter::once(start).chain(end).copied();
                self.coding_span(transcript, positions.collect())
            }
            (ResolvedTarget::Transcript(transcript), HgvsLocation::Protein { start, end }) => {
                // Codon n spans c.(3n-2) to c.3n.
                let cds = |n: u64| CodingPosition {
                    base: CdsBase::Cds(n.try_into().expect("codon numbers are nonzero")),
                    offset: 0,
                };
                let end = end.unwrap_or(*start);
                self.coding_span(
                    transcript,
                    vec![cds(3 * start.0.get() - 2), cds(3 * end.0.get())],
                )
            }
            (ResolvedTarget::Transcript(transcript), HgvsLocation::Expression(expression)) => {
                // ferro-hgvs projects a transcript description to the genome only under an
                // explicit chromosome parent, as in `NC_000012.12(NM_004985.5):c.35G>A`.
                let model = self
                    .mapper
                    .cdot()
                    .get_transcript(&transcript.0)
                    .ok_or_else(|| HgvsError::UnknownTranscript {
                        transcript: transcript.0.clone(),
                    })?;
                self.expression_span(&format!("{}({})", model.contig, transcript.0), expression)
            }
            (ResolvedTarget::Genomic(_), _) | (ResolvedTarget::Transcript(_), _) => {
                unreachable!("HgvsQuery::parse pairs chromosome targets with g. locations only")
            }
        }
    }

    /// The span covering coding positions on a transcript. Positions map strand-aware, so the
    /// span is their genomic minimum and maximum, which stays correct across exon junctions.
    fn coding_span(
        &self,
        transcript: &TranscriptAccession,
        positions: Vec<CodingPosition>,
    ) -> Result<GenomicSpan, HgvsError> {
        let model = self
            .mapper
            .cdot()
            .get_transcript(&transcript.0)
            .ok_or_else(|| HgvsError::UnknownTranscript {
                transcript: transcript.0.clone(),
            })?;
        if model.cds_start.is_none() {
            return Err(HgvsError::NonCodingTranscript {
                transcript: transcript.0.clone(),
                axis: 'c',
            });
        }
        let contig = self.contig_name(&model.contig)?;
        let mut bases = Vec::with_capacity(positions.len());
        for position in positions {
            let outside = || HgvsError::PositionOutsideTranscript {
                transcript: transcript.0.clone(),
                position: position.to_string(),
            };
            let mapped = self
                .mapper
                .cds_to_genome(&transcript.0, &cds_pos(position))
                .map_err(|_| outside())?;
            if mapped.variant.base == 0 {
                return Err(outside());
            }
            bases.push(mapped.variant.base);
        }
        Ok(GenomicSpan {
            contig,
            start: *bases
                .iter()
                .min()
                .expect("a span has at least one position"),
            end: *bases
                .iter()
                .max()
                .expect("a span has at least one position"),
        })
    }

    /// The span of a full description, such as `NM_004985.5:c.35G>A`, after normalization.
    fn expression_span(&self, accession: &str, expression: &str) -> Result<GenomicSpan, HgvsError> {
        let variant = parse_hgvs(&format!("{accession}:{expression}"))?;
        let genomic = match variant {
            HgvsVariant::Genome(_) => variant,
            _ => self.projector.project_to_genomic_normalized(&variant)?,
        };
        let HgvsVariant::Genome(genomic) = genomic else {
            unreachable!("projecting to the genome yields a g. variant")
        };
        let (start, end) =
            interval_bounds(&genomic.loc_edit.location).ok_or_else(|| HgvsError::InvalidQuery {
                input: format!("{accession}:{expression}"),
                reason: "the position is uncertain".to_string(),
            })?;
        Ok(GenomicSpan {
            contig: self.contig_name(&genomic.accession.to_string())?,
            start,
            end,
        })
    }

    /// Describes a variant on transcripts. Results are cached, so repeated hovers are cheap.
    pub fn annotate(
        &self,
        key: &VariantKey,
        scope: &AnnotationScope,
    ) -> Result<VariantHgvs, HgvsError> {
        let cache_key = (key.clone(), scope.clone());
        if let Some(cached) = self
            .annotations
            .lock()
            .expect("the cache lock is not poisoned")
            .get(&cache_key)
        {
            return Ok(cached.clone());
        }
        let mut record = VcfRecord::new(
            key.contig.clone(),
            key.pos,
            key.reference.clone(),
            vec![key.alternate.clone()],
        );
        record.genome_build = match self.build {
            HgvsBuild::Grch38 => GenomeBuild::GRCh38,
            HgvsBuild::Grch37 => GenomeBuild::GRCh37,
        };
        let variant = HgvsVariant::Genome(vcf_to_genomic_hgvs(&record, 0)?);
        let projections: Vec<VariantProjection> = match scope {
            AnnotationScope::AllOverlapping => self.projector.project_variant_all(&variant)?,
            AnnotationScope::Transcripts(transcripts) => transcripts
                .iter()
                .filter_map(|transcript| {
                    self.projector
                        .project_variant(&variant, &transcript.0)
                        .inspect_err(|e| {
                            log::debug!("Cannot project {variant} onto {}: {e}", transcript.0)
                        })
                        .ok()
                })
                .collect(),
        };
        // Projections carry the normalized g. description, which can differ from the VCF's
        // left-aligned allele.
        let genomic = projections
            .iter()
            .find_map(|projection| projection.genomic.as_ref())
            .unwrap_or(&variant)
            .to_string();
        let annotation = VariantHgvs {
            genomic,
            transcripts: projections
                .into_iter()
                .map(|projection| TranscriptHgvs {
                    region: if projection.is_intronic {
                        TranscriptRegion::Intronic
                    } else if projection.is_utr {
                        TranscriptRegion::Utr
                    } else {
                        TranscriptRegion::Exonic
                    },
                    transcript_hgvs: projection
                        .coding
                        .or(projection.noncoding)
                        .map(|variant| variant.to_string()),
                    protein: projection.protein.map(|variant| variant.to_string()),
                    transcript: projection.transcript_id,
                    gene: projection.gene_symbol,
                })
                .collect(),
        };
        self.annotations
            .lock()
            .expect("the cache lock is not poisoned")
            .put(cache_key, annotation.clone());
        Ok(annotation)
    }

    /// The `c.` position of a base on each coding transcript that covers it, such as
    /// `NM_004985.5:c.35`. Transcripts on other contigs or without a CDS are skipped.
    pub fn describe_base(
        &self,
        contig: &str,
        pos: u64,
        transcripts: &[TranscriptAccession],
    ) -> Result<Vec<String>, HgvsError> {
        let accession = self.contig_accession(contig)?;
        Ok(transcripts
            .iter()
            .filter(|transcript| {
                self.mapper
                    .cdot()
                    .get_transcript(&transcript.0)
                    .is_some_and(|model| model.contig == accession && model.cds_start.is_some())
            })
            .filter_map(|transcript| {
                let mapped = self
                    .mapper
                    .genome_to_cds(&transcript.0, &GenomePos::new(pos))
                    .ok()?;
                Some(format!("{}:c.{}", transcript.0, mapped.variant))
            })
            .collect())
    }

    /// The tgv contig name of a RefSeq chromosome accession.
    fn contig_name(&self, accession: &str) -> Result<String, HgvsError> {
        self.build
            .chromosome(accession)
            .map(|chromosome| format!("chr{chromosome}"))
            .ok_or_else(|| HgvsError::UnmappedContig {
                contig: accession.to_string(),
            })
    }

    fn contig_accession(&self, contig: &str) -> Result<&'static str, HgvsError> {
        self.build
            .contig_accession(contig)
            .ok_or_else(|| HgvsError::UnmappedContig {
                contig: contig.to_string(),
            })
    }
}

fn cds_pos(position: CodingPosition) -> CdsPos {
    let (base, utr3) = match position.base {
        CdsBase::Upstream(n) => (-(n.get() as i64), false),
        CdsBase::Cds(n) => (n.get() as i64, false),
        CdsBase::Downstream(n) => (n.get() as i64, true),
    };
    CdsPos {
        base,
        offset: (position.offset != 0).then_some(position.offset),
        utr3,
        special: None,
    }
}

/// The 1-based bounds of a genomic interval, or `None` if either end is uncertain.
fn interval_bounds(interval: &GenomeInterval) -> Option<(u64, u64)> {
    let start = interval.start.inner()?.base;
    let end = interval.end.inner()?.base;
    Some((start.min(end), start.max(end)))
}

/// The state of the worker's data.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HgvsStatus {
    Loading,
    Ready,
    Failed(String),
}

enum Request {
    Resolve(
        ResolvedQuery,
        oneshot::Sender<Result<GenomicSpan, HgvsError>>,
    ),
    Annotate(
        VariantKey,
        AnnotationScope,
        oneshot::Sender<Result<VariantHgvs, HgvsError>>,
    ),
    DescribeBase(
        String,
        u64,
        Vec<TranscriptAccession>,
        oneshot::Sender<Result<Vec<String>, HgvsError>>,
    ),
}

pub struct HgvsService;

impl HgvsService {
    /// Starts a worker thread that loads the data and then serves requests. The thread stops
    /// when every handle is dropped.
    pub fn spawn(data_dir: HgvsDataDir) -> HgvsHandle {
        let (sender, mut receiver) = mpsc::unbounded_channel::<Request>();
        let (status_sender, status) = watch::channel(HgvsStatus::Loading);
        std::thread::Builder::new()
            .name("hgvs".to_string())
            .spawn(move || {
                let engine = Engine::load(&data_dir);
                let _ = status_sender.send(match &engine {
                    Ok(_) => HgvsStatus::Ready,
                    Err(e) => HgvsStatus::Failed(e.to_string()),
                });
                while let Some(request) = receiver.blocking_recv() {
                    let unavailable = |error: &HgvsError| HgvsError::Unavailable {
                        reason: error.to_string(),
                    };
                    // A dropped reply means the caller stopped waiting, which is fine.
                    match (request, &engine) {
                        (Request::Resolve(query, reply), Ok(engine)) => {
                            let _ = reply.send(engine.resolve(&query));
                        }
                        (Request::Annotate(key, scope, reply), Ok(engine)) => {
                            let _ = reply.send(engine.annotate(&key, &scope));
                        }
                        (Request::DescribeBase(contig, pos, transcripts, reply), Ok(engine)) => {
                            let _ = reply.send(engine.describe_base(&contig, pos, &transcripts));
                        }
                        (Request::Resolve(_, reply), Err(error)) => {
                            let _ = reply.send(Err(unavailable(error)));
                        }
                        (Request::Annotate(_, _, reply), Err(error)) => {
                            let _ = reply.send(Err(unavailable(error)));
                        }
                        (Request::DescribeBase(_, _, _, reply), Err(error)) => {
                            let _ = reply.send(Err(unavailable(error)));
                        }
                    }
                }
            })
            .expect("the HGVS thread starts");
        HgvsHandle { sender, status }
    }
}

/// Sends requests to the HGVS worker. Requests sent while the data loads wait for it.
#[derive(Clone)]
pub struct HgvsHandle {
    sender: mpsc::UnboundedSender<Request>,
    pub status: watch::Receiver<HgvsStatus>,
}

impl HgvsHandle {
    async fn request<T>(
        &self,
        request: impl FnOnce(oneshot::Sender<Result<T, HgvsError>>) -> Request,
    ) -> Result<T, HgvsError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(request(reply))
            .map_err(|_| HgvsError::WorkerStopped)?;
        response.await.map_err(|_| HgvsError::WorkerStopped)?
    }

    pub async fn resolve(&self, query: ResolvedQuery) -> Result<GenomicSpan, HgvsError> {
        self.request(|reply| Request::Resolve(query, reply)).await
    }

    pub async fn annotate(
        &self,
        key: VariantKey,
        scope: AnnotationScope,
    ) -> Result<VariantHgvs, HgvsError> {
        self.request(|reply| Request::Annotate(key, scope, reply))
            .await
    }

    pub async fn describe_base(
        &self,
        contig: String,
        pos: u64,
        transcripts: Vec<TranscriptAccession>,
    ) -> Result<Vec<String>, HgvsError> {
        self.request(|reply| Request::DescribeBase(contig, pos, transcripts, reply))
            .await
    }
}
