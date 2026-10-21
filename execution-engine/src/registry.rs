use crate::models::WeatherModel;
use rustc_hash::FxHashMap;

/// Pluggable registry for weather model implementations.
///
/// Future models (WRF-Chem, WRF-Fire, etc.) simply call `register()`.
pub struct ModelRegistry {
    models: FxHashMap<String, Box<dyn WeatherModel>>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            models: FxHashMap::default(),
        }
    }

    /// Register a weather model. Replaces any existing model with the same ID.
    pub fn register(&mut self, model: Box<dyn WeatherModel>) {
        self.models.insert(model.model_id().to_string(), model);
    }

    /// Look up a model by its identifier.
    pub fn get(&self, model_id: &str) -> Option<&dyn WeatherModel> {
        self.models.get(model_id).map(|m| m.as_ref())
    }

    /// List all registered model identifiers.
    pub fn list_models(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.models.keys().cloned().collect();
        ids.sort();
        ids
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a registry pre-populated with the default models.
pub fn default_registry() -> ModelRegistry {
    let mut registry = ModelRegistry::new();
    registry.register(Box::new(crate::models::hydro_wrf::HydroWrf::default()));
    registry.register(Box::new(crate::models::soil_model::SoilModel::default()));
    registry.register(Box::new(
        crate::models::water_quality::WaterQuality::default(),
    ));
    registry.register(Box::new(crate::models::wrf_chem::WrfChem::default()));
    registry.register(Box::new(crate::models::wrf_fire::WrfFire::default()));
    registry.register(Box::new(crate::models::wrf_nano::WrfNano::default()));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_contains_all_models() {
        let registry = default_registry();
        assert!(registry.get("wrf-nano").is_some());
        assert!(registry.get("wrf-chem").is_some());
        assert!(registry.get("wrf-fire").is_some());
        assert!(registry.get("hydro-wrf").is_some());
        assert!(registry.get("soil-model").is_some());
        assert!(registry.get("water-quality").is_some());
        assert_eq!(
            registry.list_models(),
            vec![
                "hydro-wrf",
                "soil-model",
                "water-quality",
                "wrf-chem",
                "wrf-fire",
                "wrf-nano"
            ]
        );
    }

    #[test]
    fn register_and_get() {
        let mut registry = ModelRegistry::new();
        registry.register(Box::new(crate::models::wrf_nano::WrfNano::default()));
        let model = registry.get("wrf-nano").unwrap();
        assert_eq!(model.docker_image(), "wrfcoin/wrf-nano:latest");
    }

    #[test]
    fn get_missing_returns_none() {
        let registry = ModelRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn list_models_sorted() {
        let mut registry = ModelRegistry::new();
        registry.register(Box::new(crate::models::wrf_nano::WrfNano::new(
            "http://example.com".to_string(),
        )));
        let ids = registry.list_models();
        assert_eq!(ids, vec!["wrf-nano"]);
    }
}
