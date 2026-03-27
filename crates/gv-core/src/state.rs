use crate::sequence::SequenceRepositoryEnum;
use crate::tracks::{TrackService, TrackServiceEnum};
use crate::variant::VariantRepository;
use crate::{
    alignment::{Alignment, AlignmentRepositoryEnum},
    bed::{BEDInterval, BEDRepository},
    contig_header::ContigHeader,
    cytoband::Cytoband,
    error::TGVError,
    feature::Gene,
    intervals::{Focus, GenomeInterval, Region, SortedIntervalCollection},
    message::{AlignmentDisplayOption, AlignmentFilter, Movement},
    reference::Reference,
    //register::Registers,
    //rendering::{MainLayout, layout::resize_node},
    repository::Repository,
    sequence::Sequence,
    track::Track,
    variant::Variant,
};
use itertools::Itertools;

/// Holds states of the application.
pub struct State {
    pub messages: Vec<String>,

    pub contig_header: ContigHeader,
    pub reference: Reference,
    pub alignment: Alignment,
    pub alignment_options: Vec<AlignmentDisplayOption>,

    pub variants: SortedIntervalCollection<Variant>,
    pub variant_loaded: bool, // Temporary hack before proper implemetation for the indexed VCF IO

    pub bed_intervals: SortedIntervalCollection<BEDInterval>,
    pub bed_loaded: bool, // Temporary hack before proper implemetation for large bed file io

    pub track: Track<Gene>,

    pub sequence: Sequence,
}

impl State {
    pub fn new(reference: Reference, contigs: ContigHeader) -> Result<Self, TGVError> {
        Ok(Self {
            reference,

            // /settings: settings.clone(),
            messages: Vec::new(),

            alignment: Alignment::default(),
            alignment_options: Vec::new(),
            track: Track::<Gene>::default(),
            sequence: Sequence::default(),
            variants: SortedIntervalCollection::<Variant>::default(),
            variant_loaded: false,
            bed_intervals: SortedIntervalCollection::<BEDInterval>::default(),
            bed_loaded: false,
            contig_header: contigs,
        })
    }

    pub fn contig_name(&self, focus: &Focus) -> Result<&String, TGVError> {
        self.contig_header
            .try_get(focus.contig_index)
            .map(|contig| &contig.name)
    }

    pub fn current_cytoband(&self, focus: &Focus) -> Result<Option<&Cytoband>, TGVError> {
        self.contig_header
            .try_get(focus.contig_index)
            .map(|contig| contig.cytoband.as_ref())
    }

    /// Maximum length of the contig.
    pub fn contig_length(&self, focus: &Focus) -> Result<Option<u64>, TGVError> {
        self.contig_header
            .try_get(focus.contig_index)
            .map(|contig| contig.length)
    }
}

impl State {
    pub async fn movement(
        &self,
        focus: Focus,
        repository: &mut Repository,
        movement: Movement,
    ) -> Result<Focus, TGVError> {
        match movement {
            Movement::Left(n) => Ok(focus.move_left(n)),
            Movement::Right(n) => Ok(focus.move_right(n)),
            Movement::Position(position) => Ok(focus.move_to(position)),
            Movement::ContigNamePosition(contig_name, position) => Ok(Focus {
                contig_index: self
                    .contig_header
                    .try_get_index_by_str(contig_name.as_ref())?,
                position,
            }),
            Movement::NextExonsStart(n) => self.next_exons_start(focus, repository, n).await,
            Movement::NextExonsEnd(n) => self.next_exons_end(focus, repository, n).await,
            Movement::PreviousExonsStart(n) => {
                self.previous_exons_start(focus, repository, n).await
            }
            Movement::PreviousExonsEnd(n) => self.previous_exons_end(focus, repository, n).await,
            Movement::NextGenesStart(n) => self.next_genes_start(focus, repository, n).await,
            Movement::NextGenesEnd(n) => self.next_genes_end(focus, repository, n).await,
            Movement::PreviousGenesStart(n) => {
                self.previous_genes_start(focus, repository, n).await
            }
            Movement::PreviousGenesEnd(n) => self.previous_genes_end(focus, repository, n).await,

            Movement::NextContig(n) => Ok(self.next_contig(focus, n)),
            Movement::PreviousContig(n) => Ok(self.previous_contig(focus, n)),
            Movement::ContigIndex(contig_index) => Ok(Focus {
                contig_index,
                position: 1,
            }),

            Movement::Gene(name) => self.gene(repository, name.as_ref()).await,

            Movement::GoToHGVS(hgvs_str) => self.hgvs_to_focus(&hgvs_str, repository).await,

            Movement::Default => self.default_focus(repository).await,
        }
    }

    pub fn add_message(&mut self, message: String) {
        self.messages.push(message);
    }

    pub async fn load_alignment_data(
        &mut self,
        region: &Region,
        alignment_repository: &mut AlignmentRepositoryEnum,
    ) -> Result<&mut Self, TGVError> {
        // if !self.alignment.has_complete_data(&region) {
        //     Ok(false)
        // } else {
        self.alignment = alignment_repository
            .read_alignment(&region, &self.sequence, &self.contig_header)
            .await?;

        self.alignment
            .apply_options(&self.alignment_options, &self.sequence)?;

        Ok(self)
    }

    pub async fn load_track_data(
        &mut self,
        region: &Region,
        track_service: &mut TrackServiceEnum,
    ) -> Result<&mut Self, TGVError> {
        self.track = track_service
            .query_gene_track(&self.reference, &region, &self.contig_header)
            .await?;

        Ok(self)
    }

    pub async fn load_sequence_data(
        &mut self,
        region: &Region,
        sequence_repository: &mut SequenceRepositoryEnum,
    ) -> Result<&mut Self, TGVError> {
        self.sequence = sequence_repository
            .query_sequence(&region, &self.contig_header)
            .await?;

        Ok(self)
    }

    pub async fn load_variant_data(
        &mut self,
        region: &Region,
        variant_repository: &mut VariantRepository,
    ) -> Result<&mut Self, TGVError> {
        self.variants = variant_repository.read_variants(&self.contig_header)?;
        self.variant_loaded = true;
        Ok(self)
    }

    pub async fn load_bed_data(
        &mut self,
        region: &Region,
        bed_repository: &mut BEDRepository,
    ) -> Result<&mut Self, TGVError> {
        self.bed_intervals = bed_repository.read_bed(&self.contig_header)?;
        self.bed_loaded = true;
        Ok(self)
    }

    pub async fn ensure_complete_cytoband_data(
        &mut self,
        region: &Region,
        repository: &mut Repository,
    ) -> Result<bool, TGVError> {
        if self
            .contig_header
            .cytoband_is_loaded(region.contig_index())?
        {
            Ok(false)
        } else if let Some(track_service) = repository.track_service.as_mut() {
            let cytoband = track_service
                .get_cytoband(&self.reference, region.contig_index(), &self.contig_header)
                .await?;
            self.contig_header
                .try_update_cytoband(region.contig_index(), cytoband)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl State {
    /// Main function to route state message handling.
    pub fn set_alignment_change(
        &mut self,
        focus: &Focus,
        options: Vec<AlignmentDisplayOption>,
    ) -> Result<(), TGVError> {
        self.alignment.reset(&self.sequence)?;

        let options = options
            .into_iter()
            .map(|option| match option {
                AlignmentDisplayOption::Filter(AlignmentFilter::BaseAtCurrentPosition(base)) => {
                    AlignmentDisplayOption::Filter(AlignmentFilter::Base(focus.position, base))
                }

                AlignmentDisplayOption::Filter(AlignmentFilter::BaseAtCurrentPositionSoftClip) => {
                    AlignmentDisplayOption::Filter(AlignmentFilter::BaseSoftclip(focus.position))
                }

                _ => option,
            })
            .collect_vec();
        self.alignment_options = options;
        self.alignment
            .apply_options(&self.alignment_options, &self.sequence)?;

        Ok(())
    }

    //Self::get_data_requirements(state, repository)
}

// Movement handling
impl State {
    pub async fn next_genes_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        // The gene is in the track.
        if let Some(gene) = self.track.get_k_genes_after(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.start(),
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.start(),
        })
    }

    pub async fn next_genes_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(gene) = self.track.get_k_genes_after(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.end() + 1,
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.end() + 1,
        })
    }

    pub async fn previous_genes_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(gene) = self.track.get_k_genes_before(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.start() - 1,
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.start() - 1,
        })
    }

    pub async fn previous_genes_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(gene) = self.track.get_k_genes_before(focus.position, n) {
            return Ok(Focus {
                contig_index: gene.contig_index,
                position: gene.end() - 1,
            });
        }

        // Query for the target gene
        let gene = repository
            .track_service_checked()?
            .query_k_genes_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: gene.contig_index,
            position: gene.end() - 1,
        })
    }

    pub async fn next_exons_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(exon) = self.track.get_k_exons_after(focus.position, n) {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.start() + 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.start() + 1,
        })
    }

    pub async fn next_exons_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(exon) = self.track.get_k_exons_after(focus.position, n) {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.end() + 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_after(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.end() + 1,
        })
    }

    pub async fn previous_exons_start(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        if let Some(exon) = self.track.get_k_exons_before(focus.position, n) {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.start() - 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.start() - 1,
        })
    }

    pub async fn previous_exons_end(
        &self,
        focus: Focus,
        repository: &mut Repository,
        n: usize,
    ) -> Result<Focus, TGVError> {
        if n == 0 {
            return Ok(focus);
        }

        let exon = self.track.get_k_exons_before(focus.position, n);
        if let Some(exon) = exon {
            return Ok(Focus {
                contig_index: exon.contig_index,
                position: exon.end() - 1,
            });
        }

        // Query for the target exon
        let exon = repository
            .track_service_checked()?
            .query_k_exons_before(
                &self.reference,
                focus.contig_index,
                focus.position,
                n,
                &self.contig_header,
            )
            .await?;

        Ok(Focus {
            contig_index: exon.contig_index,
            position: exon.end() - 1,
        })
    }

    pub async fn gene(
        &self,
        repository: &mut Repository,
        gene_name: &str,
    ) -> Result<Focus, TGVError> {
        repository
            .track_service_checked()?
            .query_gene_name(&self.reference, gene_name, &self.contig_header)
            .await
            .map(|gene| Focus {
                contig_index: gene.contig_index(),
                position: gene.start() + 1,
            })
    }

    async fn hgvs_to_focus(
        &self,
        hgvs_str: &str,
        repository: &Repository,
    ) -> Result<Focus, TGVError> {
        match &self.reference {
            Reference::Hg38 | Reference::Hg19 => {}
            _ => {
                return Err(TGVError::StateError(
                    "HGVS navigation is only supported for HG38 and HG19 reference genomes."
                        .to_string(),
                ));
            }
        };

        let variant = ferro_hgvs::parse_hgvs(hgvs_str).map_err(|e| {
            TGVError::StateError(format!("Failed to parse HGVS variant '{}': {}", hgvs_str, e))
        })?;

        match variant {
            ferro_hgvs::HgvsVariant::Genome(genome_var) => {
                self.hgvs_genome_to_focus(&genome_var)
            }
            ferro_hgvs::HgvsVariant::Cds(cds_var) => {
                self.hgvs_cds_to_focus(&cds_var, repository)
            }
            ferro_hgvs::HgvsVariant::Protein(_) => Err(TGVError::StateError(
                "p. HGVS notation is not yet supported for navigation.".to_string(),
            )),
            _ => Err(TGVError::StateError(format!(
                "Unsupported HGVS notation type '{}'. Only g. and c. notation are supported.",
                variant.variant_type()
            ))),
        }
    }

    fn hgvs_genome_to_focus(
        &self,
        genome_var: &ferro_hgvs::hgvs::variant::GenomeVariant,
    ) -> Result<Focus, TGVError> {
        let accession = &genome_var.accession;
        let aliases = ferro_hgvs::liftover::aliases::ContigAliases::default_human();

        // Translate RefSeq accession (e.g., "NC_000017.11") to UCSC name (e.g., "chr17").
        // If the accession already carries a chromosome field (assembly-style notation),
        // use that directly. As a last resort, try the full accession string unchanged
        // in case the ContigHeader was populated with RefSeq aliases via UCSC chromAlias.
        let chr_name: String = accession
            .chromosome
            .as_deref()
            .map(|s| s.to_string())
            .or_else(|| {
                aliases
                    .refseq_to_ucsc(&accession.full())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| accession.full());

        let contig_index =
            self.contig_header
                .try_get_index_by_str(&chr_name)
                .map_err(|_| {
                    TGVError::StateError(format!(
                        "Chromosome '{}' not found in the loaded reference contigs.",
                        chr_name
                    ))
                })?;

        let position = genome_var
            .loc_edit
            .location
            .start
            .inner()
            .ok_or_else(|| {
                TGVError::StateError(
                    "HGVS variant has an uncertain or unknown start position.".to_string(),
                )
            })?
            .base;

        Ok(Focus {
            contig_index,
            position,
        })
    }

    fn hgvs_cds_to_focus(
        &self,
        cds_var: &ferro_hgvs::hgvs::variant::CdsVariant,
        repository: &Repository,
    ) -> Result<Focus, TGVError> {
        let coord_mapper =
            repository
                .cdot_mapper
                .as_ref()
                .ok_or_else(|| TGVError::StateError(
                    "Navigating to a c. HGVS variant requires a cdot transcript database. \
                     Provide one with --cdot-path <file.json.gz>."
                        .to_string(),
                ))?;

        let tx_id = cds_var.accession.full();
        let cds_pos = cds_var
            .loc_edit
            .location
            .start
            .inner()
            .ok_or_else(|| {
                TGVError::StateError(
                    "HGVS variant has an uncertain or unknown start position.".to_string(),
                )
            })?;

        let mapping = coord_mapper
            .cds_to_genome(&tx_id, cds_pos)
            .map_err(|e| {
                TGVError::StateError(format!(
                    "Failed to map c. position to genome for transcript '{}': {}",
                    tx_id, e
                ))
            })?;

        let position = mapping.variant.base;

        // Get the chromosome name from the transcript record and translate to UCSC format.
        let contig_refseq = coord_mapper
            .cdot()
            .get_transcript(&tx_id)
            .ok_or_else(|| {
                TGVError::StateError(format!(
                    "Transcript '{}' not found in the cdot database.",
                    tx_id
                ))
            })?
            .contig
            .clone();

        let aliases = ferro_hgvs::liftover::aliases::ContigAliases::default_human();
        let chr_name: String = aliases
            .refseq_to_ucsc(&contig_refseq)
            .map(|s| s.to_string())
            .unwrap_or(contig_refseq);

        let contig_index =
            self.contig_header
                .try_get_index_by_str(&chr_name)
                .map_err(|_| {
                    TGVError::StateError(format!(
                        "Chromosome '{}' from transcript '{}' not found in the loaded \
                         reference contigs.",
                        chr_name, tx_id
                    ))
                })?;

        Ok(Focus {
            contig_index,
            position,
        })
    }

    fn next_contig(&self, focus: Focus, n: usize) -> Focus {
        Focus {
            contig_index: self.contig_header.next(focus.contig_index, n),
            position: 1,
        }
    }

    fn previous_contig(&self, focus: Focus, n: usize) -> Focus {
        Focus {
            contig_index: self.contig_header.previous(focus.contig_index, n),

            position: 1,
        }
    }

    pub async fn default_focus(&self, repository: &mut Repository) -> Result<Focus, TGVError> {
        match self.reference {
            Reference::Hg38 | Reference::Hg19 => {
                return self.gene(repository, "TP53").await;
            }

            Reference::UcscGenome(_) | Reference::UcscAccession(_) => {
                // Find the first gene on the first contig. If anything is not found, handle it later.

                let first_contig = self.contig_header.first()?;

                // Try to get the first gene in the first contig.
                // We use query_k_genes_after starting from coordinate 0 with k=1.
                match repository
                    .track_service_checked()?
                    .query_k_genes_after(&self.reference, first_contig, 0, 1, &self.contig_header)
                    .await
                {
                    Ok(gene) => {
                        // Found a gene, go to its start (using 1-based coordinates for Goto)
                        return Ok(Focus {
                            contig_index: gene.contig_index,
                            position: gene.start() + 1,
                        });
                    }
                    Err(_) => {} // Gene not found. Handle later.
                }
            }

            Reference::BYOIndexedFasta(_) | Reference::BYOTwoBit(_) | Reference::NoReference => {} // handle later
        };

        // If reaches here, go to the first contig:1
        self.contig_header
            .first()
            .map(|contig_index| Focus {
                contig_index,
                position: 1,
            })
            .map_err(|_| {
                TGVError::StateError(
            "Failed to find a default initial region. Please provide a starting region with -r."
                .to_string() )
            })
    }
}
