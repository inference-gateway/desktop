use anyhow::{Context, Result, bail};
use std::fmt;
use std::str::FromStr;

/// Slice `index` of `total` (1-based) of the suite, so CI can split the tests
/// across parallel runners. Every test lands in exactly one slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shard {
    index: usize,
    total: usize,
}

impl Shard {
    /// Keeps this slice's items, taken round-robin so each slice gets a mix
    /// of the slow and the quick tests rather than one stretch of the list.
    pub fn select<T>(self, items: Vec<T>) -> Vec<T> {
        items
            .into_iter()
            .enumerate()
            .filter(|(position, _)| position % self.total == self.index - 1)
            .map(|(_, item)| item)
            .collect()
    }
}

impl FromStr for Shard {
    type Err = anyhow::Error;

    /// Parses `K/N`, rejecting a slice that is zero or past the last one.
    fn from_str(s: &str) -> Result<Self> {
        let (index, total) = s
            .split_once('/')
            .with_context(|| format!("shard {s:?} is not K/N"))?;
        let index: usize = index
            .parse()
            .with_context(|| format!("shard {s:?} has no number before the slash"))?;
        let total: usize = total
            .parse()
            .with_context(|| format!("shard {s:?} has no number after the slash"))?;
        if index == 0 || index > total {
            bail!("shard {s:?} must be between 1/N and N/N");
        }
        Ok(Self { index, total })
    }
}

impl fmt::Display for Shard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.index, self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shard_parses_as_k_of_n_and_rejects_a_slice_out_of_range() {
        for valid in ["1/1", "2/3", "3/3"] {
            assert_eq!(valid.parse::<Shard>().unwrap().to_string(), valid);
        }
        for invalid in ["0/3", "4/3", "1/0", "0/0", "3", "a/2", "1/b", "", "-1/2"] {
            assert!(invalid.parse::<Shard>().is_err(), "{invalid:?} parsed");
        }
    }

    #[test]
    fn the_shards_of_a_suite_run_every_test_exactly_once() {
        let suite: Vec<usize> = (0..11).collect();
        for total in 1..=4 {
            let mut ran: Vec<usize> = (1..=total)
                .flat_map(|index| Shard { index, total }.select(suite.clone()))
                .collect();
            ran.sort();
            assert_eq!(ran, suite, "{total} shards");
        }
    }

    #[test]
    fn a_shard_takes_every_nth_test() {
        let shard: Shard = "2/3".parse().unwrap();
        assert_eq!(shard.select((0..8).collect()), vec![1, 4, 7]);
    }
}
