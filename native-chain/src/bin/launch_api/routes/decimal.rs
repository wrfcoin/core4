//! Explicit decimal-string serialization for monetary launch-API responses.
//!
//! This is deliberately confined to HTTP response types. `Balance` keeps its
//! consensus/state serde contract; callers opt in field by field here.

use serde::{Serialize, Serializer, ser::SerializeStruct};
use wrfcoin_native_chain::blockchain::state::ContributionRewardAggregate;

pub fn u64<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

pub fn u128<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

pub fn option_u128<S>(value: &Option<u128>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(value) => serializer.serialize_some(&value.to_string()),
        None => serializer.serialize_none(),
    }
}

pub fn contribution_rewards<S>(value: &ContributionRewardAggregate, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut output = serializer.serialize_struct("ContributionRewardAggregateResponse", 7)?;
    output.serialize_field("real_paid", &value.real_paid.into_inner().to_string())?;
    output.serialize_field("real_pending", &value.real_pending.into_inner().to_string())?;
    output.serialize_field("synthetic_paid", &value.synthetic_paid.into_inner().to_string())?;
    output.serialize_field("unclassified_paid", &value.unclassified_paid.into_inner().to_string())?;
    output.serialize_field("real_decision_count", &value.real_decision_count)?;
    output.serialize_field("synthetic_decision_count", &value.synthetic_decision_count)?;
    output.serialize_field("unclassified_decision_count", &value.unclassified_decision_count)?;
    output.end()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct DecimalBoundary {
        #[serde(serialize_with = "u64")]
        u64_value: u64,
        #[serde(serialize_with = "u128")]
        u128_value: u128,
    }

    #[test]
    fn decimal_fields_are_exact_json_strings_across_js_and_integer_boundaries() {
        for value in [0, 9_007_199_254_740_991, 9_007_199_254_740_992, u64::MAX] {
            let json = serde_json::to_value(DecimalBoundary {
                u64_value: value,
                u128_value: value as u128,
            }).unwrap();
            assert_eq!(json["u64_value"].as_str().unwrap(), value.to_string());
            assert_eq!(json["u128_value"].as_str().unwrap(), value.to_string());
        }
        let max = u128::MAX;
        let json = serde_json::to_value(DecimalBoundary { u64_value: u64::MAX, u128_value: max }).unwrap();
        assert_eq!(json["u128_value"].as_str(), Some("340282366920938463463374607431768211455"));
    }

    #[test]
    fn raw_integer_mutation_fails_the_response_shape_contract() {
        #[derive(Serialize)]
        struct Protected(#[serde(serialize_with = "u128")] u128);
        let protected = serde_json::to_value(Protected(9_007_199_254_740_992)).unwrap();
        let raw = serde_json::to_value(9_007_199_254_740_992u128).unwrap();
        assert_eq!(protected.as_str(), Some("9007199254740992"));
        assert!(raw.as_str().is_none(), "raw integer serialization is not an HTTP decimal string");
    }
}
