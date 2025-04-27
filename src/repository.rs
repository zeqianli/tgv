use crate::{
    error::TGVError,
    helpers::is_url,
    models::{alignment::Alignment, contig::Contig, region::Region},
};

enum RemoteSource {
    S3,
    HTTP,
    GS,
}

trait AlignmentRepository {
    fn read_alignment(&self, region: &Region) -> Result<Alignment, TGVError>;

    fn read_header(&self) -> Result<Vec<(Contig, usize)>, TGVError>;
}

struct BAMRepository {
    bam_path: String,
    bai_path: Option<String>,
}

impl BAMRepository {
    fn new(bam_path: String, bai_path: Option<String>) -> Result<Self, TGVError> {
        if is_remote_path {
            return Err(TGVError::IOError(
                "Remote BAM files are not supported yet.".to_string(),
            ));
        }
    }

     /// Get the query string for a region.
    /// Look through the header to decide if the bam file chromosome names are abbreviated or full.
    fn get_query_contig_string(header: &Header, region: &Region) -> Result<String, TGVError> {
        let full_chromsome_str = region.contig.full_name();
        let abbreviated_chromsome_str = region.contig.abbreviated_name();

        for (_key, records) in header.to_hashmap().iter() {
            for record in records {
                if record.contains_key("SN") {
                    let reference_name = record["SN"].to_string();
                    if reference_name == full_chromsome_str {
                        return Ok(full_chromsome_str);
                    }

                    if reference_name == abbreviated_chromsome_str {
                        return Ok(abbreviated_chromsome_str);
                    }
                }
            }
        }

        Err(TGVError::IOError("Contig not found in header".to_string()))
    }
}



impl AlignmentRepository for BAMRepository {
    fn read_alignment(&self, region: &Region) -> Result<Alignment, TGVError> {
        let mut bam = match self.bai_path {
            Some(bai_path) => IndexedReader::from_path_and_index(bam_path, bai_path)?,
            None =>  IndexedReader::from_path(bam_path)?
        };

        let header = bam::Header::from_template(bam.header());

        let query_contig_string = Self::get_query_contig_string(&header, region)?;
        bam.fetch((
            &query_contig_string,
            region.start as i32 - 1,
            region.end as i32,
        ))
        .map_err(|e| TGVError::IOError(e.to_string()))?;

        let mut alignment = Alignment::new(&region.contig);
        let mut coverage_hashmap: HashMap<usize, usize> = HashMap::new(); // First use a hashmap to store coverage, then convert to BTreeMap

        for record in bam.records() {
            let read = record.map_err(|e| TGVError::IOError(e.to_string()))?;
            alignment.add_read(read);
            let aligned_read = alignment.reads.last().unwrap();

            // update coverage hashmap
            for i in aligned_read.range() {
                // TODO: check exclusivity here
                *coverage_hashmap.entry(i).or_insert(1) += 1;
            }
        }

        // Convert hashmap to BTreeMap
        for (k, v) in coverage_hashmap {
            *alignment.coverage.entry(k).or_insert(v) += v;
        }

        alignment.data_complete_left_bound = region.start;
        alignment.data_complete_right_bound = region.end;

        Ok(alignment)
    }
}

struct RemoteBAMRepository {
    bam_path: String,
    source: RemoteSouce,
}

if is_remote_path {
    IndexedReader::from_url(
        &Url::parse(bam_path).map_err(|e| TGVError::IOError(e.to_string()))?,
    )
    .unwrap()

struct CRAMRepository {
    cram_path: String,
}
