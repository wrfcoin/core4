//! Wallet-wide HTTP request admission. Not used by consensus or peer admission.
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug)]
pub struct Config { pub limit: usize, pub window_secs: u64, pub max_keys: usize }
fn setting(raw: Option<String>, fallback: u64, max: u64) -> u64 {
    raw.and_then(|v| v.parse().ok()).filter(|n| *n > 0 && *n <= max).unwrap_or(fallback)
}
impl Default for Config {
    fn default() -> Self {
        Self {
            // determinism-ok: node-local HTTP admission config, never a consensus input.
            limit: setting(std::env::var("WRFCOIN_FORECAST_QUOTA_LIMIT").ok(), 30, 1000) as usize,
            // determinism-ok: node-local HTTP admission window, never a consensus input.
            window_secs: setting(std::env::var("WRFCOIN_FORECAST_QUOTA_WINDOW_SECS").ok(), 60, 86400),
            max_keys: 10_000,
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal { RateLimited(u64), Capacity }
#[derive(Debug, Default)]
pub struct Quotas {
    pub config: Config,
    buckets: HashMap<(String, bool), VecDeque<u64>>,
}
impl Quotas {
    pub fn charge(&mut self, wallet: &str, reveal: bool, now: u64) -> Result<(), Refusal> {
        let window = self.config.window_secs;
        self.buckets.retain(|_, times| {
            while times.front().is_some_and(|t| now.saturating_sub(*t) >= window) { times.pop_front(); }
            !times.is_empty()
        });
        // The caller already enforces bearer binding. Normalize aliases here too.
        let key = (wallet.trim().to_ascii_lowercase(), reveal);
        if let Some(times) = self.buckets.get(&key) {
            if times.len() >= self.config.limit {
                return Err(Refusal::RateLimited(window.saturating_sub(now.saturating_sub(times[0])).max(1)));
            }
        } else if self.buckets.len() >= self.config.max_keys { return Err(Refusal::Capacity); }
        self.buckets.entry(key).or_default().push_back(now);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn quotas() -> Quotas { Quotas { config: Config {limit:2,window_secs:60,max_keys:4}, buckets:HashMap::new() } }
    #[test] fn aliases_share_budget_and_expiry_reopens_it() {
        let mut q=quotas();
        q.charge(" Wallet ",false,1).unwrap(); q.charge("WALLET",false,2).unwrap();
        assert_eq!(q.charge("wallet",false,3),Err(Refusal::RateLimited(58)));
        q.charge("wallet",false,61).unwrap();
    }
    #[test] fn routes_and_wallets_have_independent_budgets() {
        let mut q=quotas();
        for reveal in [false,true] {
            for _ in 0..2 { q.charge("w",reveal,1).unwrap(); }
            assert_eq!(q.charge("w",reveal,2),Err(Refusal::RateLimited(59)));
        }
        q.charge("other",false,2).unwrap();
    }
    #[test] fn capacity_never_evicts_live_keys_and_expiry_reclaims_them() {
        let mut q=quotas(); q.config.max_keys=1;
        q.charge("w",false,1).unwrap();
        assert_eq!(q.charge("other",false,2),Err(Refusal::Capacity));
        q.charge("w",false,2).unwrap();
        assert_eq!(q.charge("w",false,3),Err(Refusal::RateLimited(58)));
        q.charge("other",false,62).unwrap();
    }
    #[test] fn configuration_refuses_zero_invalid_and_unbounded_values() {
        for raw in [None,Some(""),Some("0"),Some("-1"),Some("x"),Some("1001")] {
            assert_eq!(setting(raw.map(str::to_owned),30,1000),30);
        }
        assert_eq!(setting(Some("1".into()),30,1000),1);
        assert_eq!(setting(Some("1000".into()),30,1000),1000);
    }
}
