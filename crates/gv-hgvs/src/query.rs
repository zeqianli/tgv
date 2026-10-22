//! The HGVS search grammar: `<target>:<axis>.<location>`.
//!
//! The target is a gene symbol (`KRAS`), a transcript (`NM_004985.5`), or a chromosome
//! (`NC_000012.12`). Positions without an edit, such as `KRAS:c.35` or `KRAS:p.12`, are search
//! targets rather than HGVS variants, so this module parses them itself. Anything else after the
//! axis, such as `c.35G>A`, is kept as an expression for ferro-hgvs to parse when it resolves.

use std::num::NonZeroU64;

use crate::error::HgvsError;

/// A gene symbol, such as `KRAS`.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct GeneSymbol(pub String);

/// A transcript accession, with or without a version, such as `NM_004985.5`.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct TranscriptAccession(pub String);

/// A chromosome accession, such as `NC_000012.12`.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ContigAccession(pub String);

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HgvsTarget {
    Gene(GeneSymbol),
    Transcript(TranscriptAccession),
    Genomic(ContigAccession),
}

/// The part of a coding position before any intronic offset.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CdsBase {
    /// `c.-n`: `n` bases upstream of the start codon.
    Upstream(NonZeroU64),
    /// `c.n`: the `n`-th coding base.
    Cds(NonZeroU64),
    /// `c.*n`: `n` bases downstream of the stop codon.
    Downstream(NonZeroU64),
}

/// A `c.` position, such as `c.35`, `c.-1`, `c.*12`, or `c.35+2`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CodingPosition {
    pub base: CdsBase,
    /// The intronic offset from `base`: positive downstream, negative upstream, `0` for exonic.
    pub offset: i64,
}

impl std::fmt::Display for CodingPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.base {
            CdsBase::Upstream(n) => write!(f, "c.-{n}")?,
            CdsBase::Cds(n) => write!(f, "c.{n}")?,
            CdsBase::Downstream(n) => write!(f, "c.*{n}")?,
        }
        match self.offset {
            0 => Ok(()),
            offset if offset > 0 => write!(f, "+{offset}"),
            offset => write!(f, "{offset}"),
        }
    }
}

/// A 1-based codon number, as in `p.12`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct Codon(pub NonZeroU64);

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HgvsLocation {
    /// A full description after the target, such as `c.35G>A`, for ferro-hgvs to parse.
    Expression(String),
    Coding {
        start: CodingPosition,
        end: Option<CodingPosition>,
    },
    Protein {
        start: Codon,
        end: Option<Codon>,
    },
    /// 1-based, inclusive.
    Genomic {
        start: u64,
        end: u64,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HgvsQuery {
    pub target: HgvsTarget,
    pub location: HgvsLocation,
}

impl HgvsQuery {
    /// Parses search input.
    ///
    /// Returns `None` when the input isn't HGVS, such as `chr1:100` or `TP53`, so the regular
    /// search handles it. Input is HGVS when a colon is directly followed by an axis and a dot,
    /// such as `:c.`. Returns `Some(Err(_))` for HGVS-shaped input that is invalid, so the user
    /// sees why instead of a fallback search.
    pub fn parse(input: &str) -> Option<Result<Self, HgvsError>> {
        let input = input.trim();
        let (target, axis, location) = split(input)?;
        let invalid = |reason: &str| HgvsError::InvalidQuery {
            input: input.to_string(),
            reason: reason.to_string(),
        };
        Some((|| {
            let target = parse_target(target)
                .ok_or_else(|| invalid("the target is not a gene or an accession"))?;
            let location = match (axis, &target) {
                ('g', HgvsTarget::Genomic(_)) => parse_genomic(location)
                    .unwrap_or_else(|| Ok(HgvsLocation::Expression(format!("g.{location}"))))
                    .map_err(invalid)?,
                ('g', _) => {
                    return Err(invalid(
                        "g. positions need a chromosome accession, such as NC_000012.12",
                    ));
                }
                (_, HgvsTarget::Genomic(_)) => {
                    return Err(invalid("a chromosome accession only takes g. positions"));
                }
                ('c', _) => parse_coding(location)
                    .unwrap_or_else(|| Ok(HgvsLocation::Expression(format!("c.{location}"))))
                    .map_err(invalid)?,
                ('n', _) => HgvsLocation::Expression(format!("n.{location}")),
                ('p', _) => parse_protein(location).map_err(invalid)?,
                _ => return Err(invalid("tgv searches g., c., n., and p. descriptions")),
            };
            Ok(Self { target, location })
        })())
    }
}

/// Splits `<target>:<axis>.<location>` at the first colon that an axis and a dot follow.
fn split(input: &str) -> Option<(&str, char, &str)> {
    input.match_indices(':').find_map(|(index, _)| {
        let target = &input[..index];
        let rest = &input[index + 1..];
        let mut chars = rest.chars();
        let axis = chars.next()?.to_ascii_lowercase();
        (matches!(axis, 'g' | 'c' | 'n' | 'm' | 'r' | 'p') && chars.next() == Some('.'))
            .then(|| (target, axis, &rest[2..]))
    })
}

fn parse_target(target: &str) -> Option<HgvsTarget> {
    let valid = !target.is_empty()
        && target
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if !valid {
        return None;
    }
    let prefix = |prefixes: &[&str]| prefixes.iter().any(|prefix| target.starts_with(prefix));
    Some(if prefix(&["NC_", "NT_", "NW_"]) {
        HgvsTarget::Genomic(ContigAccession(target.to_string()))
    } else if prefix(&["NM_", "NR_", "XM_", "XR_", "ENST"]) {
        HgvsTarget::Transcript(TranscriptAccession(target.to_string()))
    } else {
        HgvsTarget::Gene(GeneSymbol(target.to_string()))
    })
}

/// Splits `a_b` into its endpoints.
fn range(location: &str) -> (&str, Option<&str>) {
    match location.split_once('_') {
        Some((start, end)) => (start, Some(end)),
        None => (location, None),
    }
}

fn number(text: &str) -> Option<u64> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Parses `n` or `a_b`. Returns `None` for an expression, such as `25245350C>T`.
fn parse_genomic(location: &str) -> Option<Result<HgvsLocation, &'static str>> {
    let (start, end) = range(location);
    let start = number(start)?;
    let end = match end {
        Some(end) => number(end)?,
        None => start,
    };
    Some(if start == 0 {
        Err("positions start at 1")
    } else if start > end {
        Err("the range ends before it starts")
    } else {
        Ok(HgvsLocation::Genomic { start, end })
    })
}

/// Parses a `c.` position or range. Returns `None` for an expression, such as `35G>A`.
fn parse_coding(location: &str) -> Option<Result<HgvsLocation, &'static str>> {
    let (start, end) = range(location);
    let position = |text| coding_position(text).transpose();
    let start = match position(start)? {
        Ok(start) => start,
        Err(reason) => return Some(Err(reason)),
    };
    let end = match end.map(position) {
        Some(None) => return None,
        Some(Some(Err(reason))) => return Some(Err(reason)),
        Some(Some(Ok(end))) => Some(end),
        None => None,
    };
    Some(Ok(HgvsLocation::Coding { start, end }))
}

/// Parses `[-|*]n[(+|-)offset]`. Returns `Ok(None)` when the text isn't a position.
fn coding_position(text: &str) -> Result<Option<CodingPosition>, &'static str> {
    let (kind, rest) = match text.as_bytes().first() {
        Some(b'-') => (CdsBase::Upstream as fn(NonZeroU64) -> CdsBase, &text[1..]),
        Some(b'*') => (CdsBase::Downstream as fn(NonZeroU64) -> CdsBase, &text[1..]),
        _ => (CdsBase::Cds as fn(NonZeroU64) -> CdsBase, text),
    };
    let (base, offset) = match rest.find(['+', '-']) {
        Some(index) => (&rest[..index], Some(&rest[index..])),
        None => (rest, None),
    };
    let Some(base) = number(base) else {
        return Ok(None);
    };
    let offset = match offset {
        Some(offset) => match number(&offset[1..]) {
            Some(distance) if offset.starts_with('-') => -(distance as i64),
            Some(distance) => distance as i64,
            None => return Ok(None),
        },
        None => 0,
    };
    let base = NonZeroU64::new(base).ok_or("c. positions have no c.0")?;
    Ok(Some(CodingPosition {
        base: kind(base),
        offset,
    }))
}

/// Parses `n` or `a_b` codon numbers.
fn parse_protein(location: &str) -> Result<HgvsLocation, &'static str> {
    const CODON_ONLY: &str = "protein search takes a codon number, such as KRAS:p.12";
    let codon = |text: &str| {
        let number = number(text).ok_or(CODON_ONLY)?;
        NonZeroU64::new(number)
            .map(Codon)
            .ok_or("codons start at 1")
    };
    let (start, end) = range(location);
    let start = codon(start)?;
    let end = end.map(codon).transpose()?;
    if end.is_some_and(|end| end < start) {
        return Err("the range ends before it starts");
    }
    Ok(HgvsLocation::Protein { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn nonzero(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap()
    }

    fn cds(n: u64, offset: i64) -> CodingPosition {
        CodingPosition {
            base: CdsBase::Cds(nonzero(n)),
            offset,
        }
    }

    fn gene(symbol: &str) -> HgvsTarget {
        HgvsTarget::Gene(GeneSymbol(symbol.to_string()))
    }

    fn transcript(accession: &str) -> HgvsTarget {
        HgvsTarget::Transcript(TranscriptAccession(accession.to_string()))
    }

    fn genomic(accession: &str) -> HgvsTarget {
        HgvsTarget::Genomic(ContigAccession(accession.to_string()))
    }

    fn protein(start: u64, end: Option<u64>) -> HgvsLocation {
        HgvsLocation::Protein {
            start: Codon(nonzero(start)),
            end: end.map(|end| Codon(nonzero(end))),
        }
    }

    fn coding(start: CodingPosition, end: Option<CodingPosition>) -> HgvsLocation {
        HgvsLocation::Coding { start, end }
    }

    #[rstest]
    #[case("KRAS:p.12", gene("KRAS"), protein(12, None))]
    #[case("KRAS:p.12_13", gene("KRAS"), protein(12, Some(13)))]
    #[case("KRAS:P.12", gene("KRAS"), protein(12, None))]
    #[case(" KRAS:p.12 ", gene("KRAS"), protein(12, None))]
    #[case("HLA-A:p.5", gene("HLA-A"), protein(5, None))]
    #[case("NM_004985.5:p.12", transcript("NM_004985.5"), protein(12, None))]
    #[case("NM_004985:p.12", transcript("NM_004985"), protein(12, None))]
    #[case("KRAS:c.35", gene("KRAS"), coding(cds(35, 0), None))]
    #[case("KRAS:c.35+2", gene("KRAS"), coding(cds(35, 2), None))]
    #[case("KRAS:c.36-1", gene("KRAS"), coding(cds(36, -1), None))]
    #[case("KRAS:c.34_36", gene("KRAS"), coding(cds(34, 0), Some(cds(36, 0))))]
    #[case(
        "KRAS:c.-1",
        gene("KRAS"),
        coding(CodingPosition { base: CdsBase::Upstream(nonzero(1)), offset: 0 }, None),
    )]
    #[case(
        "KRAS:c.*12",
        gene("KRAS"),
        coding(CodingPosition { base: CdsBase::Downstream(nonzero(12)), offset: 0 }, None),
    )]
    #[case(
        "KRAS:c.-5-3",
        gene("KRAS"),
        coding(CodingPosition { base: CdsBase::Upstream(nonzero(5)), offset: -3 }, None),
    )]
    #[case(
        "NC_000012.12:g.25245350",
        genomic("NC_000012.12"),
        HgvsLocation::Genomic { start: 25245350, end: 25245350 },
    )]
    #[case(
        "NC_000012.12:g.25245349_25245351",
        genomic("NC_000012.12"),
        HgvsLocation::Genomic { start: 25245349, end: 25245351 },
    )]
    #[case(
        "NM_004985.5:c.35G>A",
        transcript("NM_004985.5"),
        HgvsLocation::Expression("c.35G>A".to_string()),
    )]
    #[case(
        "KRAS:c.35_36delinsTT",
        gene("KRAS"),
        HgvsLocation::Expression("c.35_36delinsTT".to_string()),
    )]
    #[case(
        "NC_000012.12:g.25245350C>T",
        genomic("NC_000012.12"),
        HgvsLocation::Expression("g.25245350C>T".to_string()),
    )]
    #[case(
        "NR_003051.4:n.100",
        transcript("NR_003051.4"),
        HgvsLocation::Expression("n.100".to_string()),
    )]
    fn parses(#[case] input: &str, #[case] target: HgvsTarget, #[case] location: HgvsLocation) {
        let query = HgvsQuery::parse(input)
            .expect("the input is HGVS")
            .expect("the input is valid");
        assert_eq!(query, HgvsQuery { target, location });
    }

    #[rstest]
    #[case("KRAS:p.0")]
    #[case("KRAS:p.13_12")]
    #[case("KRAS:p.")]
    #[case("KRAS:p.G12")]
    #[case("KRAS:p.Gly12")]
    #[case("KRAS:p.G12D")]
    #[case("KRAS:p.12del")]
    #[case("KRAS:c.0")]
    #[case("KRAS:c.34_0")]
    #[case("NC_000012.12:g.0")]
    #[case("NC_000012.12:g.20_10")]
    #[case("NC_000012.12:c.35")]
    #[case("KRAS:g.25245350")]
    #[case("KRAS:r.35")]
    #[case("NC_012920.1:m.100")]
    #[case("BAD SYMBOL:c.35")]
    #[case(":c.35")]
    fn rejects(#[case] input: &str) {
        assert!(matches!(
            HgvsQuery::parse(input),
            Some(Err(HgvsError::InvalidQuery { .. }))
        ));
    }

    #[rstest]
    #[case("chr1:100")]
    #[case("chr1:1,000-2,000")]
    #[case("1000-2000")]
    #[case("HLA-A*01:01")]
    #[case("HLA-A*01:01:01:01")]
    #[case("TP53")]
    #[case("1234")]
    #[case("chrX:c")]
    fn ignores_non_hgvs(#[case] input: &str) {
        assert!(HgvsQuery::parse(input).is_none());
    }
}
