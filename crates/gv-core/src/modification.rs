/// Base modification types as defined in the SAM MM/ML tag specification.
/// Reference: https://samtools.github.io/hts-specs/SAMtags.pdf
use std::collections::HashMap;

use crate::error::TGVError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModificationType {
    /// 5-methylcytosine (5mC), encoded as C+m in the MM tag
    FiveMC,
    /// 5-hydroxymethylcytosine (5hmC), encoded as C+h in the MM tag
    FiveHMC,
    /// N6-methyladenine (6mA), encoded as A+a in the MM tag
    SixMA,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseModification {
    pub modification_type: ModificationType,
    /// Probability from the ML tag: 0 = unmodified, 255 = fully modified.
    pub probability: u8,
}

impl BaseModification {
    /// Probability > 70% (probability >= 179/255).
    pub fn is_high(&self) -> bool {
        self.probability >= 179
    }

    /// Probability < 30% (probability < 77/255).
    pub fn is_low(&self) -> bool {
        self.probability < 77
    }
}

/// Parse MM and ML auxiliary tags from raw BAM record data bytes.
///
/// Returns a map from 1-based reference positions to a list of base modifications
/// at that position.
///
/// # Arguments
/// * `mm_str`   – The raw MM tag string value (e.g. `"C+m?,0,3,1;C+h?,2"`)
/// * `ml_bytes` – Raw ML byte array (probabilities 0-255 in MM order)
/// * `seq`      – Read sequence bases (uppercase, matching SAM query)
/// * `cigar_ops` – Pre-parsed CIGAR operations (from noodles)
/// * `alignment_start` – 1-based alignment start on the reference
pub fn parse_modification_data(
    mm_str: &str,
    ml_bytes: &[u8],
    seq: &[u8],
    cigar_ops: &[noodles::sam::alignment::record::cigar::Op],
    alignment_start: u64,
) -> Result<HashMap<u64, Vec<BaseModification>>, TGVError> {
    use noodles::sam::alignment::record::cigar::op::Kind;

    let mut result: HashMap<u64, Vec<BaseModification>> = HashMap::new();

    // Build query-position (0-based) → reference-position (1-based) mapping.
    // CIGAR soft-clips are included in the query but not the reference.
    let mut q_to_r: HashMap<usize, u64> = HashMap::new();
    {
        let mut q_cursor: usize = 0;
        let mut r_cursor: u64 = alignment_start;

        for op in cigar_ops {
            let len = op.len();
            match op.kind() {
                Kind::SoftClip | Kind::Insertion => {
                    q_cursor += len;
                }
                Kind::HardClip | Kind::Pad => {}
                Kind::Deletion | Kind::Skip => {
                    r_cursor += len as u64;
                }
                Kind::Match | Kind::SequenceMatch | Kind::SequenceMismatch => {
                    for i in 0..len {
                        q_to_r.insert(q_cursor + i, r_cursor + i as u64);
                    }
                    q_cursor += len;
                    r_cursor += len as u64;
                }
            }
        }
    }

    // For each base type (A/C/G/T), collect query positions (0-based) in order.
    let mut base_positions: HashMap<u8, Vec<usize>> = HashMap::new();
    for (i, &base) in seq.iter().enumerate() {
        let upper = base.to_ascii_uppercase();
        base_positions.entry(upper).or_default().push(i);
    }

    // Parse the MM tag.  Format (per SAM spec):
    //   MM:Z:{base}{strand}{code}[.?],delta,delta,...[;...]
    // Multiple modification types are separated by ';'.
    let mut ml_cursor: usize = 0;

    for section in mm_str.trim_end_matches(';').split(';') {
        let section = section.trim();
        if section.is_empty() {
            continue;
        }

        // Split header from deltas at first comma.
        let (header, deltas_str) = if let Some(pos) = section.find(',') {
            (&section[..pos], &section[pos + 1..])
        } else {
            (section, "")
        };

        // Header must be at least "{base}{strand}{code}", e.g. "C+m" (3 bytes).
        let hdr = header.as_bytes();
        if hdr.len() < 3 {
            continue;
        }

        let base = hdr[0].to_ascii_uppercase();
        let strand = hdr[1];

        // Only handle forward-strand modifications (+).
        if strand != b'+' {
            // Still advance ml_cursor past any probabilities for this section.
            let n = if deltas_str.is_empty() {
                0
            } else {
                deltas_str.split(',').count()
            };
            ml_cursor += n;
            continue;
        }

        // Single-letter modification code (or ChEBI ID – skip those for now).
        let mod_code = hdr[2];
        let mod_type = match mod_code {
            b'm' => ModificationType::FiveMC,
            b'h' => ModificationType::FiveHMC,
            b'a' => ModificationType::SixMA,
            _ => {
                let n = if deltas_str.is_empty() {
                    0
                } else {
                    deltas_str.split(',').count()
                };
                ml_cursor += n;
                continue;
            }
        };

        let positions = match base_positions.get(&base) {
            Some(p) => p,
            None => continue,
        };

        if deltas_str.is_empty() {
            continue;
        }

        let mut pos_cursor: usize = 0;

        for delta_str in deltas_str.split(',') {
            let delta: usize = match delta_str.trim().parse() {
                Ok(d) => d,
                Err(_) => break,
            };

            pos_cursor += delta;
            if pos_cursor >= positions.len() {
                ml_cursor += 1;
                continue;
            }

            let query_pos = positions[pos_cursor];
            pos_cursor += 1;

            let probability = ml_bytes.get(ml_cursor).copied().unwrap_or(255);
            ml_cursor += 1;

            if let Some(&ref_pos) = q_to_r.get(&query_pos) {
                result.entry(ref_pos).or_default().push(BaseModification {
                    modification_type: mod_type.clone(),
                    probability,
                });
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use noodles::sam::alignment::record::cigar::{Op, op::Kind};

    fn make_match(len: usize) -> Op {
        Op::new(Kind::Match, len)
    }

    fn make_softclip(len: usize) -> Op {
        Op::new(Kind::SoftClip, len)
    }

    fn make_deletion(len: usize) -> Op {
        Op::new(Kind::Deletion, len)
    }

    /// Simple all-M CIGAR: read = ACGCACGC, ref start = 1
    /// MM: C+m?,0,1 → first C (pos 1), skip 1, third C (pos 5)
    /// ML: [200, 50]
    #[test]
    fn test_5mc_basic() {
        // Read: A C G C A C G C  (0-based query: 0,1,2,3,4,5,6,7)
        //                              C positions: 1, 3, 5, 7
        // MM: C+m?,0,1 → delta 0: take C[0]=query 1 → ref 2; delta 1: skip 1, take C[2]=query 5 → ref 6
        let seq = b"ACGCACGC";
        let cigars = vec![make_match(8)];
        let mm = "C+m?,0,1";
        let ml = vec![200u8, 50u8];

        let result = parse_modification_data(mm, &ml, seq, &cigars, 1).unwrap();

        // ref_pos 2 (query 1, 1st C): probability 200
        assert!(result.contains_key(&2));
        assert_eq!(result[&2][0].modification_type, ModificationType::FiveMC);
        assert_eq!(result[&2][0].probability, 200);

        // ref_pos 6 (query 5, 3rd C): probability 50
        assert!(result.contains_key(&6));
        assert_eq!(result[&6][0].modification_type, ModificationType::FiveMC);
        assert_eq!(result[&6][0].probability, 50);

        // Only 2 positions modified
        assert_eq!(result.len(), 2);
    }

    /// Test with a leading soft-clip: query positions include soft-clipped bases.
    /// Read: [SS]ACGCACGC  (soft-clip of 2, then 8 match)
    /// C positions in full query (0-based): 3, 5, 7, 9
    #[test]
    fn test_5mc_with_softclip() {
        let seq = b"GGACGCACGC"; // 2 soft-clipped + 8 aligned
        let cigars = vec![make_softclip(2), make_match(8)];
        let mm = "C+m?,0";
        let ml = vec![255u8];

        let result = parse_modification_data(mm, &ml, seq, &cigars, 1).unwrap();

        // C positions in query: 3, 5, 7, 9 (0-based, including soft-clip)
        // delta 0 → take C[0] = query 3 → ref_pos = 1 + (3-2) = 2
        assert!(result.contains_key(&2));
        assert_eq!(result[&2][0].probability, 255);
        assert_eq!(result.len(), 1);
    }

    /// Test with 5hmC modifications.
    #[test]
    fn test_5hmc() {
        let seq = b"ACGCACGC";
        let cigars = vec![make_match(8)];
        let mm = "C+h?,1";
        let ml = vec![180u8];

        let result = parse_modification_data(mm, &ml, seq, &cigars, 1).unwrap();

        // C positions: query 1, 3, 5, 7 → delta 1 skips 1, takes C[1]=query 3 → ref 4
        assert!(result.contains_key(&4));
        assert_eq!(result[&4][0].modification_type, ModificationType::FiveHMC);
        assert_eq!(result[&4][0].probability, 180);
        assert_eq!(result.len(), 1);
    }

    /// Test that unknown modification codes are skipped without panic.
    #[test]
    fn test_unknown_mod_code_is_skipped() {
        let seq = b"ACGCACGC";
        let cigars = vec![make_match(8)];
        // 'z' is not a recognised mod code
        let mm = "C+z?,0";
        let ml = vec![200u8];

        let result = parse_modification_data(mm, &ml, seq, &cigars, 1).unwrap();
        assert!(result.is_empty());
    }

    /// Test that reverse-strand sections are skipped.
    #[test]
    fn test_reverse_strand_skipped() {
        let seq = b"ACGCACGC";
        let cigars = vec![make_match(8)];
        let mm = "C-m?,0";
        let ml = vec![200u8];

        let result = parse_modification_data(mm, &ml, seq, &cigars, 1).unwrap();
        assert!(result.is_empty());
    }

    /// Test empty MM string produces empty result without panic.
    #[test]
    fn test_empty_mm() {
        let seq = b"ACGT";
        let cigars = vec![make_match(4)];
        let result = parse_modification_data("", &[], seq, &cigars, 1).unwrap();
        assert!(result.is_empty());
    }

    /// Test with deletion in CIGAR: deleted positions have no query base.
    #[test]
    fn test_with_deletion() {
        // Read seq: ACGCAC (6 bases, 0-based: A=0,C=1,G=2,C=3,A=4,C=5)
        // CIGAR: 3M 2D 3M, alignment start = 1
        // q→r mapping: q0→r1, q1→r2, q2→r3, (2D: r4,r5 skipped), q3→r6, q4→r7, q5→r8
        // C positions in query (0-based): 1, 3, 5
        // MM C+m?,0,0: delta 0 → take C[0]=q1→r2; delta 0 → take C[1]=q3→r6
        let seq = b"ACGCAC";
        let cigars = vec![make_match(3), make_deletion(2), make_match(3)];
        let mm = "C+m?,0,0";
        let ml = vec![200u8, 100u8];

        let result = parse_modification_data(mm, &ml, seq, &cigars, 1).unwrap();

        assert!(result.contains_key(&2), "expected ref pos 2 (1st C)");
        assert!(result.contains_key(&6), "expected ref pos 6 (2nd C, after deletion)");
        assert_eq!(result[&2][0].probability, 200);
        assert_eq!(result[&6][0].probability, 100);
        assert_eq!(result.len(), 2);
    }
}
