use std::ops::{Add, AddAssign};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// Subset of `input_tokens` served from a provider prompt cache.
    #[serde(default)]
    pub cached_input_tokens: u64,
}

impl Usage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

impl Add for Usage {
    type Output = Usage;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            input_tokens: self.input_tokens + rhs.input_tokens,
            output_tokens: self.output_tokens + rhs.output_tokens,
            cached_input_tokens: self.cached_input_tokens + rhs.cached_input_tokens,
        }
    }
}

impl AddAssign for Usage {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

/// Prices are in USD per million tokens. They are deliberately user-supplied:
/// hard-coded vendor prices go stale and silently wrong cost displays are worse
/// than no cost display.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Pricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    #[serde(default)]
    pub cached_input_per_mtok: f64,
}

impl Pricing {
    pub fn cost(&self, usage: Usage) -> f64 {
        let cached = usage.cached_input_tokens.min(usage.input_tokens);
        let fresh = usage.input_tokens.saturating_sub(cached);
        (fresh as f64 * self.input_per_mtok
            + cached as f64 * self.cached_input_per_mtok
            + usage.output_tokens as f64 * self.output_per_mtok)
            / 1_000_000.0
    }
}

#[derive(Debug, Clone)]
pub struct CostTracker {
    pricing: Option<Pricing>,
    pub usage: Usage,
    pub cost_usd: f64,
    max_cost_usd: Option<f64>,
}

impl CostTracker {
    pub fn new(pricing: Option<Pricing>, max_cost_usd: Option<f64>) -> Self {
        Self {
            pricing,
            usage: Usage::default(),
            cost_usd: 0.0,
            max_cost_usd,
        }
    }

    pub fn pricing(&self) -> Option<Pricing> {
        self.pricing
    }

    pub fn record(&mut self, usage: Usage) -> Result<()> {
        self.usage += usage;
        if let Some(pricing) = self.pricing {
            self.cost_usd += pricing.cost(usage);
        }
        if let Some(max) = self.max_cost_usd {
            if self.cost_usd > max {
                return Err(Error::Budget(format!(
                    "session cost ${:.4} exceeded limit ${:.4}",
                    self.cost_usd, max
                )));
            }
        }
        Ok(())
    }

    pub fn cost_known(&self) -> bool {
        self.pricing.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_tokens_are_billed_at_cache_rate() {
        let pricing = Pricing {
            input_per_mtok: 10.0,
            output_per_mtok: 20.0,
            cached_input_per_mtok: 1.0,
        };
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cached_input_tokens: 400_000,
        };
        // 600k * 10 + 400k * 1 + 500k * 20 = 6 + 0.4 + 10 = 16.4
        assert!((pricing.cost(usage) - 16.4).abs() < 1e-9);
    }

    #[test]
    fn budget_is_enforced() {
        let pricing = Pricing {
            input_per_mtok: 1.0,
            output_per_mtok: 1.0,
            cached_input_per_mtok: 0.0,
        };
        let mut tracker = CostTracker::new(Some(pricing), Some(0.000001));
        assert!(tracker
            .record(Usage {
                input_tokens: 10,
                output_tokens: 0,
                cached_input_tokens: 0,
            })
            .is_err());
    }
}
