use crate::tracks::{TrackCache, TrackService, TRACK_PREFERENCES};
use crate::{
    contig::Contig,
    cytoband::{Cytoband, CytobandSegment},
    error::TGVError,
    feature::{Gene, SubGeneFeature},
    reference::Reference,
    region::Region,
    strand::Strand,
    track::Track,
    traits::GenomeInterval,
};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::de::Error as _;
use serde::Deserialize;
use sqlx::{Column, Row};

// TODO: improved pattern:
// Service doesn't save anything. No reference, no cache.
// Ask these things to be passed in. And return them to store in the state.

#[derive(Debug)]
pub struct UcscApiTrackService {
    client: Client,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
struct GeneResponse1 {
    name: String,
    name2: String,

    strand: String,

    txStart: usize,
    txEnd: usize,
    cdsStart: usize,
    cdsEnd: usize,
    exonStarts: String,
    exonEnds: String,
}

impl GeneResponse1 {
    /// Custom deserializer for strand field
    fn gene(self, contig: &Contig) -> Result<Gene, TGVError> {
        Ok(Gene {
            id: self.name,
            name: self.name2,
            strand: Strand::from_str(self.strand)?,
            contig: contig.clone(),
            transcription_start: self.txStart,
            transcription_end: self.txEnd,
            cds_start: self.cdsStart,
            cds_end: self.cdsEnd,
            exon_starts: parse_comma_separated_list(&self.exonStarts)?,
            exon_ends: parse_comma_separated_list(&self.exonEnds)?,
            has_exons: true,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
struct GeneResponse2 {
    name: String,

    strand: String,
    txStart: usize,
    txEnd: usize,
    cdsStart: usize,
    cdsEnd: usize,
    exonStarts: String,
    exonEnds: String,
}

impl GeneResponse2 {
    /// Custom deserializer for strand field
    fn gene(self, contig: &Contig) -> Result<Gene, TGVError> {
        Ok(Gene {
            id: self.name.clone(),
            name: self.name.clone(),
            strand: Strand::from_str(self.strand)?,
            contig: contig.clone(),
            transcription_start: self.txStart,
            transcription_end: self.txEnd,
            cds_start: self.cdsStart,
            cds_end: self.cdsEnd,
            exon_starts: parse_comma_separated_list(&self.exonStarts)?,
            exon_ends: parse_comma_separated_list(&self.exonEnds)?,
            has_exons: true,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(non_snake_case)]
struct GeneResponse3 {
    /*

    Example responsse:

    {
    "chrom": "NC_072398.2",
    "chromStart": 130929426,
    "chromEnd": 130985030,
    "name": "NM_001142759.1",
    "score": 0,
    "strand": "+",
    "thickStart": 130929440,
    "thickEnd": 130982945,
    "reserved": "0",
    "blockCount": 13,
    "blockSizes": "65,124,76,182,122,217,167,126,78,192,72,556,374,",
    "chromStarts": "0,8926,14265,18877,31037,33561,34781,36127,39014,43150,43484,53351,55230,",
    "name2": "DBT",
    "cdsStartStat": "cmpl",
    "cdsEndStat": "cmpl",
    "exonFrames": "0,0,1,2,1,0,1,0,0,0,0,0,-1,",
    "type": "",
    "geneName": "NM_001142759.1",
    "geneName2": "DBT",
    "geneType": ""
    }

    I'm not sure if the implementation is correct.

    */
    chromStart: usize,
    chromEnd: usize,
    name: String,
    strand: String,
    thickStart: usize,
    thickEnd: usize,
}

impl GeneResponse3 {
    /// TODO: I'm not sure if this is correct.
    fn gene(self, contig: &Contig) -> Result<Gene, TGVError> {
        Ok(Gene {
            id: self.name.clone(),
            name: self.name.clone(),
            strand: Strand::from_str(self.strand)?,
            contig: contig.clone(),
            transcription_start: self.chromStart,
            transcription_end: self.chromEnd,
            cds_start: self.thickStart,
            cds_end: self.thickEnd,
            exon_starts: vec![],
            exon_ends: vec![],
            has_exons: false,
        })
    }
}

/// Custom deserializer for comma-separated lists in UCSC response
fn parse_comma_separated_list(s: &str) -> Result<Vec<usize>, TGVError> {
    s.trim_end_matches(',')
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|num| {
            num.parse::<usize>()
                .map_err(|_| TGVError::ValueError(format!("Failed to parse {}", num)))
        })
        .collect()
}

impl UcscApiTrackService {
    pub fn new() -> Result<Self, TGVError> {
        Ok(Self {
            client: Client::new(),
        })
    }

    /// Query the API to download the gene track data for a contig.
    pub async fn query_track_by_contig(
        &self,
        reference: &Reference,
        contig: &Contig,
        cache: &mut TrackCache,
    ) -> Result<Track<Gene>, TGVError> {
        if cache.get_preferred_track_name().is_none() {
            let preferred_track = self
                .get_preferred_track_name(reference, cache)
                .await?
                .ok_or(TGVError::IOError(format!(
                    "Failed to get prefered track for {} from UCSC API",
                    contig.name
                )))?; // TODO: proper handling

            cache.set_preferred_track_name(Some(preferred_track));
        }

        let preferred_track = cache.get_preferred_track_name().unwrap();

        let preferred_track = preferred_track.ok_or(TGVError::IOError(format!(
            "Failed to get prefered track for {} from UCSC API",
            contig.name
        )))?;

        let query_url = self
            .get_track_data_url(reference, contig, preferred_track.clone(), cache)
            .await?;

        let response_value: serde_json::Value =
            self.client.get(query_url).send().await?.json().await?;

        let genes_array_value = response_value.get(&preferred_track).ok_or_else(|| {
            TGVError::JsonSerializationError(serde_json::Error::custom(format!(
                "Track key \'{}\' not found in UCSC API response. Full response: {:?}",
                preferred_track, response_value
            )))
        })?;

        let mut genes: Vec<Gene> = Vec::new();
        let mut deserialized_successfully = false;

        // Attempt 1: GeneResponse1
        if let Ok(gene_responses) =
            serde_json::from_value::<Vec<GeneResponse1>>(genes_array_value.clone())
        {
            for gr in gene_responses {
                genes.push(gr.gene(contig)?);
            }
            deserialized_successfully = true;
        }

        // Attempt 2: GeneResponse2
        if !deserialized_successfully {
            if let Ok(gene_responses) =
                serde_json::from_value::<Vec<GeneResponse2>>(genes_array_value.clone())
            {
                for gr in gene_responses {
                    genes.push(gr.gene(contig)?);
                }
                deserialized_successfully = true;
            }
        }

        // Attempt 3: Direct Gene deserialization (handles complex format via GeneHelper in feature.rs)
        if !deserialized_successfully {
            if let Ok(gene_responses) =
                serde_json::from_value::<Vec<GeneResponse3>>(genes_array_value.clone())
            {
                for gr in gene_responses {
                    genes.push(gr.gene(contig)?);
                }
                deserialized_successfully = true;
            }
        }

        if !deserialized_successfully {
            return Err(TGVError::JsonSerializationError(serde_json::Error::custom(
                format!(
                    "Failed to deserialize gene data from UCSC API for track \'{}\' using any known format. Gene array value: {:?}",
                    preferred_track, genes_array_value
                )
            )));
        }

        Track::from_genes(genes, contig.clone())
    }

    const CYTOBAND_TRACK: &str = "cytoBandIdeo";

    async fn get_track_data_url(
        &self,
        reference: &Reference,
        contig: &Contig,
        track_name: String,
        cache: &mut TrackCache,
    ) -> Result<String, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => Ok(format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
                reference.to_string(),
                track_name,
                contig.name
            )),
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                Ok(format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track={}&chrom={}",
                    hub_url, genome, track_name, contig.name
                ))
            }
        }
    }

    pub async fn get_hub_url_for_genark_accession(
        &self,
        accession: &str,
    ) -> Result<String, TGVError> {
        let query_url = format!(
            "https://api.genome.ucsc.edu/list/genarkGenomes?genome={}",
            accession
        );
        let response = self.client.get(query_url).send().await?;

        // Example response:
        // {
        //     "downloadTime": "2025:05:06T03:46:07Z",
        //     "downloadTimeStamp": 1746503167,
        //     "dataTime": "2025-04-29T10:42:00",
        //     "dataTimeStamp": 1745948520,
        //     "hubUrlPrefix": "/gbdb/genark",
        //     "genarkGenomes": {
        //       "GCF_028858775.2": {
        //         "hubUrl": "GCF/028/858/775/GCF_028858775.2/hub.txt",
        //         "asmName": "NHGRI_mPanTro3-v2.0_pri",
        //         "scientificName": "Pan troglodytes",
        //         "commonName": "chimpanzee (v2 AG18354 primary hap 2024 refseq)",
        //         "taxId": 9598,
        //         "priority": 138,
        //         "clade": "primates"
        //       }
        //     },
        //     "totalAssemblies": 5691,
        //     "itemsReturned": 1
        //   }

        let response_text = response.text().await?;
        let value: serde_json::Value = serde_json::from_str(&response_text)?;

        Ok(format!(
            "https://hgdownload.soe.ucsc.edu/hubs/{}",
            value["genarkGenomes"][accession]["hubUrl"]
                .as_str()
                .ok_or(TGVError::IOError(format!(
                    "Failed to get hub url for {}",
                    accession
                )))?
        ))
    }
}

/// Get the preferred track name recursively.
fn get_all_track_names(content: &serde_json::Value) -> Result<Vec<String>, TGVError> {
    let err = TGVError::IOError("Failed to get genome from UCSC API".to_string());

    let mut names = Vec::new();

    for (key, value) in content.as_object().ok_or(err)?.iter() {
        if value.get("compositeContainer").is_some() {
            // do this recursively
            names.extend(get_all_track_names(value)?);
        } else {
            names.push(key.to_string());
        }
    }

    Ok(names)
}

fn get_preferred_track_name_from_vec(names: &Vec<String>) -> Result<Option<String>, TGVError> {
    for pref in TRACK_PREFERENCES {
        if names.contains(&pref.to_string()) {
            return Ok(Some(pref.to_string()));
        }
    }

    Ok(None)
}

#[async_trait]
impl TrackService for UcscApiTrackService {
    async fn close(&self) -> Result<(), TGVError> {
        // reqwest client dones't need closing
        Ok(())
    }

    async fn get_all_contigs(
        &self,
        reference: &Reference,
        track_cache: &mut TrackCache,
    ) -> Result<Vec<(Contig, usize)>, TGVError> {
        match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => {
                let query_url = format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?genome={}",
                    reference.to_string()
                );

                let response = self.client.get(query_url).send().await?.text().await?;

                let err = TGVError::IOError(format!(
                    "Failed to deserialize chromosomes for {}",
                    reference.to_string()
                ));

                // schema: {..., "chromosomes": [{"__name__", len}]}

                let value: serde_json::Value = serde_json::from_str(&response)?;

                let mut output = Vec::new();
                for (k, v) in value["chromosomes"].as_object().ok_or(err)?.iter() {
                    // TODO: save length
                    output.push((Contig::new(k), v.as_u64().unwrap() as usize));
                }

                output.sort_by(|(a, length_a), (b, length_b)| {
                    if a.name.starts_with("chr") || b.name.starts_with("chr") {
                        Contig::contigs_compare(a, b)
                    } else {
                        length_b.cmp(length_a) // Sort by length in descending order
                    }
                });

                Ok(output)
            }
            Reference::UcscAccession(genome) => {
                if track_cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    track_cache.hub_url = Some(hub_url);
                }
                let hub_url = track_cache.hub_url.as_ref().unwrap();

                let query_url = format!(
                    "https://api.genome.ucsc.edu/list/chromosomes?hubUrl={};genome={}",
                    hub_url, genome
                );

                let response = self
                    .client
                    .get(query_url)
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

                let mut output = Vec::new();

                for (k, v) in response
                    .get("chromosomes")
                    .ok_or(TGVError::IOError(format!(
                        "Failed to parse response for chromosomes for UCSC accession {}. Response: {:?}",
                        genome, response
                    )))?
                    .as_object()
                    .ok_or(TGVError::IOError(format!(
                        "Failed to parse response for chromosomes for UCSC accession {}. Response: {:?}",
                        genome, response
                    )))?
                    .iter()
                {
                    output.push((
                        Contig::new(k),
                        v.as_u64()
                            .ok_or(TGVError::IOError(format!(
                                "Failed to get contig {} length for UCSC accession {}. Response: {:?}",
                                k, genome, response
                            )))?
                            as usize,
                    ));
                }

                // Longest contig first
                // These contigs are likely not well-named, so longest contig first.
                // Note that this is different from the UCSC assemblies.
                output.sort_by(|(a, a_len), (b, b_len)| b_len.cmp(a_len));

                Ok(output)
            }
        }
    }

    async fn get_cytoband(
        &self,
        reference: &Reference,
        contig: &Contig,
        cache: &mut TrackCache,
    ) -> Result<Option<Cytoband>, TGVError> {
        let query_url = match reference {
            Reference::Hg19 | Reference::Hg38 | Reference::UcscGenome(_) => format!(
                "https://api.genome.ucsc.edu/getData/track?genome={}&track={}&chrom={}",
                reference.to_string(),
                Self::CYTOBAND_TRACK,
                contig.name
            ),
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self.get_hub_url_for_genark_accession(genome).await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                format!(
                    "https://api.genome.ucsc.edu/getData/track?hubUrl={}&genome={}&track={}&chrom={}",
                    hub_url, genome, Self::CYTOBAND_TRACK, contig.name
                )
            }
        };

        let response = self.client.get(query_url).send().await?;

        if response.status() != StatusCode::OK {
            return Ok(None); // Some genome doesn't have cytobands
        }

        let response_text = response.text().await?;
        let value: serde_json::Value =
            serde_json::from_str(&response_text).map_err(TGVError::JsonSerializationError)?;

        // Extract the array of segments from the "cytoBandIdeo" field
        let segments_value = value.get(Self::CYTOBAND_TRACK).ok_or_else(|| {
            TGVError::JsonSerializationError(serde_json::Error::custom(format!(
                "Missing '{}' field in UCSC API response",
                Self::CYTOBAND_TRACK
            )))
        })?;

        // Deserialize the segments array
        let segments: Vec<CytobandSegment> = serde_json::from_value(segments_value.clone())
            .map_err(TGVError::JsonSerializationError)?;

        if segments.is_empty() {
            return Ok(None);
        }

        // Construct the *single* Cytoband object for this contig
        let cytoband = Cytoband {
            reference: Some(reference.clone()),
            contig: contig.clone(),
            segments,
        };

        // Return the single Cytoband wrapped in Option
        Ok(Some(cytoband))
    }

    async fn get_preferred_track_name(
        &self,
        reference: &Reference,
        cache: &mut TrackCache,
    ) -> Result<Option<String>, TGVError> {
        match reference.clone() {
            Reference::Hg19 | Reference::Hg38 => Ok(Some("ncbiRefSeqSelect".to_string())),
            Reference::UcscGenome(genome) => {
                let query_url =
                    format!("https://api.genome.ucsc.edu/list/tracks?genome={}", genome);
                let response = reqwest::get(query_url)
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

                let track_names = get_all_track_names(response.get(genome.clone()).ok_or(
                    TGVError::IOError("Failed to get genome from UCSC API".to_string()),
                )?)?;

                let prefered_track = get_preferred_track_name_from_vec(&track_names)?;

                Ok(prefered_track)
            }
            Reference::UcscAccession(genome) => {
                if cache.hub_url.is_none() {
                    let hub_url = self
                        .get_hub_url_for_genark_accession(genome.as_str())
                        .await?;
                    cache.hub_url = Some(hub_url);
                }
                let hub_url = cache.hub_url.as_ref().unwrap();
                let query_url = format!(
                    "https://api.genome.ucsc.edu/list/tracks?hubUrl={}&genome={}",
                    hub_url, genome
                );
                let response = reqwest::get(query_url)
                    .await?
                    .json::<serde_json::Value>()
                    .await?;
                let track_names = get_all_track_names(response.get(genome.clone()).ok_or(
                    TGVError::IOError("Failed to get genome from UCSC API".to_string()),
                )?)?;

                let prefered_track = get_preferred_track_name_from_vec(&track_names)?;
                Ok(prefered_track)
            }
        }
    }

    async fn query_genes_overlapping(
        &self,
        reference: &Reference,
        region: &Region,
        cache: &mut TrackCache,
    ) -> Result<Vec<Gene>, TGVError> {
        if !cache.includes_contig(region.contig()) {
            let track = self
                .query_track_by_contig(reference, region.contig(), cache)
                .await?;
            cache.add_track(region.contig(), Some(track));
        }
        // TODO: now I don't really handle empty query results

        if let Some(Some(track)) = cache.get_track(region.contig()) {
            Ok(track
                .get_features_overlapping(region)
                .iter()
                .map(|g| (*g).clone())
                .collect())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                region.contig().name
            )))
        }
    }

    async fn query_gene_covering(
        &self,
        reference: &Reference,
        contig: &Contig,
        position: usize,
        cache: &mut TrackCache,
    ) -> Result<Option<Gene>, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track.get_gene_at(position).map(|g| (*g).clone()))
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_gene_name(
        &self,
        reference: &Reference,
        name: &String,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if !cache.includes_gene(name) {
            // query all possible tracks until the gene is found
            for (contig, _) in self.get_all_contigs(reference, cache).await? {
                let track = self
                    .query_track_by_contig(reference, &contig, cache)
                    .await?;
                cache.add_track(&contig, Some(track));

                if let Some(Some(gene)) = cache.get_gene(name) {
                    break;
                }
            }
        }

        if let Some(Some(gene)) = cache.get_gene(name) {
            Ok(gene.clone())
        } else {
            Err(TGVError::IOError(format!("Gene {} not found", name)))
        }
    }

    async fn query_k_genes_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }

        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_genes_after(coord, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_k_genes_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<Gene, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_genes_before(coord, k)
                .ok_or(TGVError::IOError("No genes found".to_string()))?
                .clone())
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_k_exons_after(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_exons_after(coord, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }

    async fn query_k_exons_before(
        &self,
        reference: &Reference,
        contig: &Contig,
        coord: usize,
        k: usize,
        cache: &mut TrackCache,
    ) -> Result<SubGeneFeature, TGVError> {
        if !cache.includes_contig(contig) {
            let track = self.query_track_by_contig(reference, contig, cache).await?;
            cache.add_track(contig, Some(track));
        }
        if let Some(Some(track)) = cache.get_track(contig) {
            Ok(track
                .get_saturating_k_exons_before(coord, k)
                .ok_or(TGVError::IOError("No exons found".to_string()))?)
        } else {
            Err(TGVError::IOError(format!(
                "Track not found for contig {}",
                contig.name
            )))
        }
    }
}
