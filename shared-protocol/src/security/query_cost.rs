//! #2137 — bounded query cost: pagination, historical range and simulation limits.
//!
//! A query handler is charged for the **work it performs**, not the rows it
//! returns. [`CostMeter::charge`] is called as the handler traverses; when the
//! scan-unit budget or the wall-time deadline is exhausted the handler aborts
//! mid-query with [`CostExceeded`] instead of finishing a wide, low-yield scan.
//!
//! The other three limits are decided *before* any work:
//!
//! * [`page_size`] rejects a page larger than `max_page_size` rather than
//!   silently truncating it, so a caller cannot mistake a clamp for a full page;
//! * [`HistoryRange::bounded`] rejects a block range wider than
//!   `max_history_blocks` before the first record is read;
//! * [`declared_inputs_within`] rejects a request whose *declared* input count
//!   exceeds the configured maximum before anything is allocated for it — the
//!   #1833 lesson, where the allocation itself was the attack.
//!
//! [`Cursor`] is an opaque continuation token. Deep pagination with an offset is
//! a quadratic scan (page *k* re-walks *k−1* pages); a cursor names the position
//! the previous page ended at, so every page costs the same.

use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The limits a node applies to query work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryCost {
    /// Largest page a caller may request; larger is rejected, not clamped.
    pub max_page_size: u32,
    /// Scan units one query may spend (a unit is one record examined).
    pub max_scan_units: u64,
    /// Widest historical block range one query may address.
    pub max_history_blocks: u64,
    /// Wall-clock deadline for one query's traversal.
    pub max_wall_time: Duration,
}

impl Default for QueryCost {
    fn default() -> Self {
        Self {
            max_page_size: 500,
            max_scan_units: 50_000,
            max_history_blocks: 1_000_000,
            max_wall_time: Duration::from_secs(2),
        }
    }
}

/// Why a traversal was aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostExceeded {
    ScanUnits { spent: u64, max: u64 },
    WallTime { elapsed: Duration, max: Duration },
}

impl fmt::Display for CostExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScanUnits { spent, max } => {
                write!(f, "query examined {spent} records, limit {max}")
            },
            Self::WallTime { elapsed, max } => {
                write!(
                    f,
                    "query ran {} ms, limit {} ms",
                    elapsed.as_millis(),
                    max.as_millis()
                )
            },
        }
    }
}

impl std::error::Error for CostExceeded {}

/// Charges work as a handler traverses; aborts when the budget is gone.
#[derive(Debug, Clone)]
pub struct CostMeter {
    spent: u64,
    max_units: u64,
    started: Instant,
    max_wall: Duration,
}

impl CostMeter {
    pub fn new(cost: &QueryCost) -> Self {
        Self {
            spent: 0,
            max_units: cost.max_scan_units,
            // determinism-ok: API read-query traversal deadline only; the clock never enters consensus state, a block, or a signed preimage.
            started: Instant::now(),
            max_wall: cost.max_wall_time,
        }
    }

    /// Charge `units` of work. Fails once the total exceeds the budget or the
    /// deadline has passed — checked on every charge, so a pathological query
    /// cannot hold a worker past the deadline between two checks.
    pub fn charge(&mut self, units: u64) -> Result<(), CostExceeded> {
        self.spent = self.spent.saturating_add(units);
        if self.spent > self.max_units {
            return Err(CostExceeded::ScanUnits {
                spent: self.spent,
                max: self.max_units,
            });
        }
        self.check_deadline()
    }

    pub fn check_deadline(&self) -> Result<(), CostExceeded> {
        let elapsed = self.started.elapsed();
        if elapsed > self.max_wall {
            return Err(CostExceeded::WallTime {
                elapsed,
                max: self.max_wall,
            });
        }
        Ok(())
    }

    pub fn spent(&self) -> u64 {
        self.spent
    }

    pub fn remaining(&self) -> u64 {
        self.max_units.saturating_sub(self.spent)
    }
}

/// A page-size request that cannot be honoured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageError {
    Zero,
    TooLarge { requested: u32, max: u32 },
}

impl fmt::Display for PageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => write!(f, "limit must be greater than 0"),
            Self::TooLarge { requested, max } => {
                write!(f, "limit {requested} exceeds the maximum page size {max}")
            },
        }
    }
}

/// Resolve a requested page size: `None` → `default`, `0` and anything above
/// `max_page_size` rejected. Never truncates.
pub fn page_size(limit: Option<u32>, default: u32, cost: &QueryCost) -> Result<u32, PageError> {
    let requested = limit.unwrap_or(default);
    if requested == 0 {
        return Err(PageError::Zero);
    }
    if requested > cost.max_page_size {
        return Err(PageError::TooLarge {
            requested,
            max: cost.max_page_size,
        });
    }
    Ok(requested)
}

/// A historical range that cannot be served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeError {
    Inverted { from: u64, to: u64 },
    TooWide { span: u64, max: u64 },
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inverted { from, to } => write!(f, "from_block {from} is after to_block {to}"),
            Self::TooWide { span, max } => write!(f, "range spans {span} blocks, limit {max}"),
        }
    }
}

/// An inclusive block range, bounded before anything is scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRange {
    pub from: u64,
    pub to: u64,
}

impl HistoryRange {
    /// `to` defaults to `tip`; `from` defaults to the start of the widest
    /// permitted window ending at `to`. An explicit range wider than
    /// `max_history_blocks`, or inverted, is refused — nothing has been read yet.
    pub fn bounded(
        from: Option<u64>,
        to: Option<u64>,
        tip: u64,
        cost: &QueryCost,
    ) -> Result<Self, RangeError> {
        let max = cost.max_history_blocks.max(1);
        let to = to.unwrap_or(tip);
        let from = from.unwrap_or_else(|| to.saturating_sub(max - 1));
        if from > to {
            return Err(RangeError::Inverted { from, to });
        }
        let span = (to - from).saturating_add(1);
        if span > max {
            return Err(RangeError::TooWide { span, max });
        }
        Ok(Self { from, to })
    }

    pub fn contains(&self, height: u64) -> bool {
        (self.from..=self.to).contains(&height)
    }

    pub fn span(&self) -> u64 {
        (self.to - self.from).saturating_add(1)
    }
}

/// Opaque continuation token. Encodes the position the previous page ended at
/// with a check byte, so a mangled or hand-built value is refused rather than
/// misread as a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cursor(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorError(pub String);

impl fmt::Display for CursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cursor is not a continuation token this node issued")
    }
}

impl Cursor {
    const PREFIX: &'static str = "c1";

    fn check(position: u64) -> u8 {
        position
            .to_le_bytes()
            .iter()
            .fold(0x5au8, |acc, b| acc.wrapping_mul(31).wrapping_add(*b))
    }

    pub fn encode(self) -> String {
        format!(
            "{}.{:016x}.{:02x}",
            Self::PREFIX,
            self.0,
            Self::check(self.0)
        )
    }

    pub fn decode(token: &str) -> Result<Self, CursorError> {
        let mut parts = token.trim().split('.');
        let (Some(prefix), Some(hex), Some(check), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(CursorError(token.to_string()));
        };
        if prefix != Self::PREFIX || hex.len() != 16 || check.len() != 2 {
            return Err(CursorError(token.to_string()));
        }
        let position = u64::from_str_radix(hex, 16).map_err(|_| CursorError(token.to_string()))?;
        let expected = u8::from_str_radix(check, 16).map_err(|_| CursorError(token.to_string()))?;
        if expected != Self::check(position) {
            return Err(CursorError(token.to_string()));
        }
        Ok(Self(position))
    }
}

/// A declared input count above the configured maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputsExceeded {
    pub declared: usize,
    pub max: usize,
}

impl fmt::Display for InputsExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "request declares {} inputs, limit {}",
            self.declared, self.max
        )
    }
}

/// Check a request's declared input count **before** allocating for it.
pub fn declared_inputs_within(declared: usize, max: usize) -> Result<(), InputsExceeded> {
    if declared > max {
        Err(InputsExceeded { declared, max })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost() -> QueryCost {
        QueryCost {
            max_page_size: 100,
            max_scan_units: 1_000,
            max_history_blocks: 10_000,
            max_wall_time: Duration::from_millis(50),
        }
    }

    #[test]
    fn meter_bounds_work_not_rows() {
        let mut m = CostMeter::new(&cost());
        for _ in 0..1_000 {
            m.charge(1).unwrap();
        }
        assert_eq!(m.spent(), 1_000);
        assert_eq!(m.remaining(), 0);
        assert_eq!(
            m.charge(1),
            Err(CostExceeded::ScanUnits {
                spent: 1_001,
                max: 1_000
            })
        );
    }

    #[test]
    fn meter_aborts_a_wide_low_yield_scan_mid_query() {
        // 5,000 records, one match every 100: an unmetered scan reads them all.
        let mut m = CostMeter::new(&cost());
        let mut matched = 0;
        let mut examined = 0;
        let result = (0..5_000u64).try_for_each(|i| {
            examined += 1;
            m.charge(1)?;
            if i % 100 == 0 {
                matched += 1;
            }
            Ok::<(), CostExceeded>(())
        });
        assert!(matches!(result, Err(CostExceeded::ScanUnits { .. })));
        assert_eq!(examined, 1_001, "aborted at the budget, not at the end");
        assert!(matched < 50);
    }

    #[test]
    fn meter_enforces_the_wall_time_deadline() {
        let mut m = CostMeter::new(&QueryCost {
            max_wall_time: Duration::from_millis(1),
            ..cost()
        });
        std::thread::sleep(Duration::from_millis(5));
        assert!(matches!(m.charge(1), Err(CostExceeded::WallTime { .. })));
        assert!(matches!(
            m.check_deadline(),
            Err(CostExceeded::WallTime { .. })
        ));
    }

    #[test]
    fn page_size_rejects_rather_than_truncates() {
        let c = cost();
        assert_eq!(page_size(None, 50, &c), Ok(50));
        assert_eq!(page_size(Some(100), 50, &c), Ok(100));
        assert_eq!(
            page_size(Some(101), 50, &c),
            Err(PageError::TooLarge {
                requested: 101,
                max: 100
            })
        );
        assert_eq!(page_size(Some(0), 50, &c), Err(PageError::Zero));
    }

    #[test]
    fn history_range_is_bounded_before_any_scan() {
        let c = cost();
        assert_eq!(
            HistoryRange::bounded(Some(10), Some(20), 100, &c),
            Ok(HistoryRange { from: 10, to: 20 })
        );
        assert_eq!(
            HistoryRange::bounded(None, None, 100, &c),
            Ok(HistoryRange { from: 0, to: 100 })
        );
        assert_eq!(
            HistoryRange::bounded(None, None, 50_000, &c),
            Ok(HistoryRange {
                from: 40_001,
                to: 50_000
            })
        );
        assert_eq!(
            HistoryRange::bounded(Some(0), Some(50_000), 50_000, &c),
            Err(RangeError::TooWide {
                span: 50_001,
                max: 10_000
            })
        );
        assert_eq!(
            HistoryRange::bounded(Some(0), Some(u64::MAX), 100, &c),
            Err(RangeError::TooWide {
                span: u64::MAX,
                max: 10_000
            })
        );
        assert_eq!(
            HistoryRange::bounded(Some(30), Some(20), 100, &c),
            Err(RangeError::Inverted { from: 30, to: 20 })
        );
        let r = HistoryRange::bounded(Some(5), Some(9), 100, &c).unwrap();
        assert_eq!(r.span(), 5);
        assert!(r.contains(5) && r.contains(9) && !r.contains(10));
    }

    #[test]
    fn cursor_round_trips_and_refuses_hand_built_tokens() {
        for position in [0u64, 1, 12_345, u64::MAX] {
            let token = Cursor(position).encode();
            assert_eq!(Cursor::decode(&token), Ok(Cursor(position)), "{token}"); // secret-leak-ok: test; token is a public pagination cursor encoding, not a bearer credential.
        }
        assert!(Cursor::decode("").is_err());
        assert!(Cursor::decode("12345").is_err());
        assert!(
            Cursor::decode("c1.0000000000003039.00").is_err(),
            "wrong check byte"
        );
        let token = Cursor(12_345).encode();
        let mangled = token.replace("3039", "3040");
        assert!(Cursor::decode(&mangled).is_err());
    }

    #[test]
    fn deep_pagination_by_cursor_costs_the_same_as_the_first_page() {
        // A cursor names the position; the handler slices from it and charges
        // only what it examines. Page 1 and page 1,000 cost `page` units each.
        let records: Vec<u64> = (0..100_000).collect();
        let page = 100usize;
        let mut cost_of = |start: u64| {
            let mut m = CostMeter::new(&cost());
            let items: Vec<u64> = records
                .iter()
                .skip(start as usize)
                .take(page)
                .inspect(|_| m.charge(1).unwrap())
                .copied()
                .collect();
            assert_eq!(items.len(), page);
            m.spent()
        };
        let first = cost_of(Cursor::decode(&Cursor(0).encode()).unwrap().0);
        let deep = cost_of(Cursor::decode(&Cursor(99_000).encode()).unwrap().0);
        assert_eq!(first, deep);
        assert_eq!(first, page as u64);
    }

    #[test]
    fn declared_inputs_are_checked_before_allocation() {
        assert_eq!(declared_inputs_within(16, 16), Ok(()));
        assert_eq!(
            declared_inputs_within(17, 16),
            Err(InputsExceeded {
                declared: 17,
                max: 16
            })
        );
        assert_eq!(
            declared_inputs_within(usize::MAX, 16),
            Err(InputsExceeded {
                declared: usize::MAX,
                max: 16
            })
        );
    }
}
