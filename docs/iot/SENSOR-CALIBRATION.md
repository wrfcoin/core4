# Sensor Calibration Standards and Thresholds

## Overview

This document details the sensor calibration standards, validation thresholds, and data quality
requirements for the WRFCoin weather data collection network. All thresholds are based on
internationally recognized meteorological standards.

## Standards Compliance

### Reference Standards

| Standard | Organization | Description |
|----------|--------------|-------------|
| WMO CIMO Guide | World Meteorological Organization | Guide to Meteorological Instruments and Methods of Observation |
| ISO 2533:1975 | International Organization for Standardization | Standard Atmosphere |
| NOAA/NWS Obs | National Oceanic and Atmospheric Administration | Observation Standards |
| AMS Standards | American Meteorological Society | Meteorological Measurement Standards |

---

## Temperature Calibration

### Measurement Accuracy Requirements

| Parameter | Value | Source |
|-----------|-------|--------|
| Accuracy (Class 1) | ±0.1°C | WMO CIMO Guide |
| Precision | ±0.05°C | High-quality sensors |
| Calibration Uncertainty | ±0.2°C | Standard calibration process |

### Physical Limits

| Parameter | Min | Max | Notes |
|-----------|-----|-----|-------|
| Extreme Range | -89.4°C | +56.7°C | Earth's recorded extremes |
| Operational Range | -60.0°C | +50.0°C | Typical station limits |

**Record References:**
- Minimum: -89.4°C at Vostok Station, Antarctica (July 21, 1983)
- Maximum: +56.7°C at Death Valley, USA (July 10, 1913)

### Rate of Change Validation

| Condition | Max Rate | Notes |
|-----------|----------|-------|
| Typical Diurnal | 5.0°C/hour | Normal day/night variation |
| Storm Conditions | 10.0°C/hour | During severe weather |
| Extreme Events | 20.0°C/hour | Maximum realistic rate |
| Microclimate Effects | 15.0°C/hour | Urban heat island |

### Spatial Consistency Tolerances

| Comparison Type | Tolerance | Notes |
|-----------------|-----------|-------|
| Same Elevation | 3.0°C | Within same elevation band |
| Per 100m Elevation | 0.65°C | Standard lapse rate |
| Urban-Rural | 5.0°C | Heat island allowance |
| Coastal | 8.0°C | Land-sea temperature gradient |

---

## Atmospheric Pressure Calibration

### Measurement Accuracy Requirements

| Parameter | Value | Source |
|-----------|-------|--------|
| Accuracy | ±0.1 hPa | Precision barometers |
| Precision | ±0.05 hPa | Research-grade instruments |
| Calibration Uncertainty | ±0.3 hPa | Standard calibration |

### Physical Limits (Sea Level Equivalent)

| Parameter | Min | Max | Notes |
|-----------|-----|-----|-------|
| Extreme Range | 870.0 hPa | 1085.7 hPa | Record extremes |
| Operational Range | 900.0 hPa | 1050.0 hPa | Typical conditions |

**Record References:**
- Minimum: 870.0 hPa during Typhoon Tip (October 12, 1979)
- Maximum: 1085.7 hPa at Agata, Siberia (December 31, 1968)

### Standard Atmosphere (ISO 2533)

| Parameter | Value | Notes |
|-----------|-------|-------|
| Sea Level Pressure | 1013.25 hPa | Standard reference |
| Tolerance | ±1.0 hPa | Standard conditions |
| Lapse Rate | 12.0 hPa/100m | Standard atmosphere |

### Rate of Change Validation

| Condition | Max Rate | Notes |
|-----------|----------|-------|
| Typical Tendency | 2.0 hPa/hour | Normal conditions |
| Storm Development | 6.0 hPa/hour | Cyclone formation |
| Extreme Events | 12.0 hPa/hour | Hurricane pressure falls |
| Microseismic | 0.1 hPa/hour | High-frequency oscillations |

---

## Humidity Calibration

### Measurement Accuracy Requirements

| Parameter | Value | Source |
|-----------|-------|--------|
| Accuracy | ±3.0% RH | Standard sensors (WMO) |
| Precision | ±1.0% RH | Quality sensors |
| Calibration Uncertainty | ±5.0% RH | Standard calibration |
| Hysteresis Effect | ±2.0% RH | Sensor characteristics |

### Physical Limits

| Parameter | Min | Max | Notes |
|-----------|-----|-----|-------|
| Absolute Range | 0.0% | 100.0% | Physical limits |
| Indoor Operational | 10.0% | 90.0% | Practical limits |
| Outdoor Operational | 5.0% | 95.0% | Practical limits |

### Psychrometric Consistency

| Parameter | Tolerance | Notes |
|-----------|-----------|-------|
| Temp-Dewpoint Calculation | 5.0% RH | RH calculation tolerance |
| Vapor Pressure | 0.5 kPa | Agreement check |
| Mixing Ratio | 1.0 g/kg | Agreement check |
| Near Saturation | 3.0% RH | Special tolerance |

### Rate of Change Validation

| Condition | Max Rate | Notes |
|-----------|----------|-------|
| Typical Variation | 10.0%/hour | Normal conditions |
| Frontal Passage | 25.0%/hour | Weather fronts |
| Extreme Events | 40.0%/hour | Maximum realistic rate |
| Dewfall/Evaporation | 15.0%/hour | Phase changes |

---

## Wind Calibration

### Measurement Accuracy Requirements

| Parameter | Value | Source |
|-----------|-------|--------|
| Speed Accuracy | ±0.5 m/s | WMO Standard |
| Speed Precision | ±0.1 m/s | Quality instruments |
| Direction Accuracy | ±5.0° | WMO Standard |
| Direction Precision | ±1.0° | Quality instruments |
| Gust Accuracy | ±1.0 m/s | WMO Standard |

### Physical Limits

| Parameter | Min | Max | Notes |
|-----------|-----|-----|-------|
| Speed Range | 0.0 m/s | 150.0 m/s | Extreme hurricane winds |
| Operational Limit | 0.0 m/s | 100.0 m/s | Typical anemometer range |
| Gust Threshold | 3.0 m/s | - | Minimum for gusts |
| Direction Range | 0.0° | 359.9° | Compass range |

### Gust Factor Validation

| Parameter | Min | Typical | Max | Extreme |
|-----------|-----|---------|-----|---------|
| Gust Factor | 1.0 | 1.3 | 3.0 | 5.0 |

### Rate of Change Validation

| Parameter | Max Rate | Notes |
|-----------|----------|-------|
| Speed Change | 10.0 m/s/min | Rapid acceleration |
| Direction Change | 45.0°/min | Wind shift |
| Gust Duration | 3.0 seconds | Gust definition |

---

## Dewpoint Calibration

### Measurement Accuracy Requirements

| Parameter | Value | Notes |
|-----------|-------|-------|
| Accuracy | ±0.3°C | Standard sensors |
| Precision | ±0.1°C | High-quality sensors |
| Calibration Uncertainty | ±0.5°C | Standard calibration |

### Physical Constraints

| Parameter | Min | Max | Notes |
|-----------|-----|-----|-------|
| Extreme Range | -80.0°C | +35.0°C | Theoretical limits |
| Max Exceeding Temp | 0.1°C | - | Sensor uncertainty |

### Temperature-Dewpoint Relationship

| Condition | Typical Spread | Notes |
|-----------|----------------|-------|
| Humid Conditions | 2.0°C | Temp - Dewpoint |
| Arid Conditions | 30.0°C | Max spread in dry air |

---

## Solar Radiation Calibration

### Measurement Accuracy Requirements

| Parameter | Value | Notes |
|-----------|-------|-------|
| Accuracy | ±50 W/m² | Typical pyranometer |
| Precision | ±10 W/m² | Quality instruments |
| Calibration Uncertainty | ±25 W/m² | Standard calibration |
| Cosine Response Error | ±5% | Angular response |

### Physical Limits

| Parameter | Value | Notes |
|-----------|-------|-------|
| Theoretical Maximum | 1400 W/m² | Top of atmosphere |
| Clear Sky Maximum | 1200 W/m² | Sea level |
| Typical Maximum | 1000 W/m² | Most locations |
| Night Threshold | 10 W/m² | Noise level |

### Net Radiation Limits

| Parameter | Min | Max | Notes |
|-----------|-----|-----|-------|
| Net Radiation | -250 W/m² | +800 W/m² | Full range |
| Nighttime Maximum | - | +50 W/m² | Thermal radiation |

---

## Precipitation Calibration

### Measurement Accuracy Requirements

| Parameter | Value | Notes |
|-----------|-------|-------|
| Accumulation Accuracy | ±0.1 mm | Tipping bucket |
| Precision | ±0.05 mm | High-quality gauges |
| Wind Undercatch | 10% | Estimated loss |
| Evaporation Loss | 0.2 mm/hr | Open gauges |

### Intensity Classification

| Classification | Intensity | Notes |
|----------------|-----------|-------|
| Light | < 2.5 mm/hr | Drizzle/light rain |
| Moderate | 2.5-10.0 mm/hr | Steady rain |
| Heavy | 10.0-50.0 mm/hr | Heavy rain |
| Extreme | > 50.0 mm/hr | Severe weather |

### Record Extremes

| Period | Maximum | Location | Date |
|--------|---------|----------|------|
| 1-hour | 305.0 mm | Foc-Foc, Réunion | 1966 |
| 24-hour | 1825.0 mm | Foc-Foc, Réunion | 1966 |

---

## Validation Profiles

### Research Profile (Strictest)

| Parameter | Tolerance | Confidence Target |
|-----------|-----------|-------------------|
| Temperature | ±0.1°C | 0.99 |
| Pressure | ±0.1 hPa | 0.99 |
| Humidity | ±1.0% RH | 0.99 |
| Wind Speed | ±0.5 m/s | 0.99 |
| Radiation | ±10 W/m² | 0.99 |

### Operational Profile (Standard)

| Parameter | Tolerance | Confidence Target |
|-----------|-----------|-------------------|
| Temperature | ±0.5°C | 0.95 |
| Pressure | ±1.0 hPa | 0.95 |
| Humidity | ±5.0% RH | 0.95 |
| Wind Speed | ±2.0 m/s | 0.95 |
| Radiation | ±50 W/m² | 0.95 |

### Climate Profile (Long-term Monitoring)

| Parameter | Tolerance | Confidence Target |
|-----------|-----------|-------------------|
| Temperature | ±0.2°C | 0.97 |
| Pressure | ±0.5 hPa | 0.97 |
| Humidity | ±3.0% RH | 0.97 |
| Wind Speed | ±1.0 m/s | 0.97 |
| Radiation | ±25 W/m² | 0.97 |

### Real-time Profile (Fastest)

| Parameter | Tolerance | Confidence Target |
|-----------|-----------|-------------------|
| Temperature | ±1.0°C | 0.90 |
| Pressure | ±2.0 hPa | 0.90 |
| Humidity | ±10.0% RH | 0.90 |
| Wind Speed | ±3.0 m/s | 0.90 |
| Radiation | ±100 W/m² | 0.90 |

### Aviation Profile (Safety Critical)

| Parameter | Tolerance | Confidence Target |
|-----------|-----------|-------------------|
| Temperature | ±0.3°C | 0.98 |
| Pressure | ±0.3 hPa | 0.98 |
| Humidity | ±5.0% RH | 0.98 |
| Wind Speed | ±1.0 m/s | 0.98 |
| Radiation | ±50 W/m² | 0.98 |

---

## Data Quality Control

### Completeness Requirements

| Quality Level | Completeness | Notes |
|---------------|--------------|-------|
| Minimum | 75% | Acceptable data |
| Good | 90% | Quality data |
| Excellent | 95% | High-quality data |

### Consistency Checks

| Check Type | Required Percentage | Notes |
|------------|---------------------|-------|
| Temporal | 95% | Time series consistency |
| Spatial | 85% | Geographic consistency |
| Physics | 90% | Physical relationship validation |

### Outlier Detection

| Method | Threshold | Notes |
|--------|-----------|-------|
| Z-Score | 3.5 | Standard deviations |
| IQR Multiplier | 1.5 | Interquartile range |
| Ensemble Agreement | 60% | Multi-sensor agreement |

---

## Environmental Adjustments

### Altitude Corrections

| Adjustment | Value | Notes |
|------------|-------|-------|
| Temperature | -0.65°C/100m | Standard lapse rate |
| Pressure | -12.0 hPa/100m | Standard atmosphere |
| Humidity | -2%/100m | Approximate adjustment |

### Climate Zone Adjustments

| Climate Zone | Temperature Range | Humidity Range | Precip Max |
|--------------|-------------------|----------------|------------|
| Tropical | 15-40°C | 40-95% | 100 mm/hr |
| Temperate | -30-45°C | 20-90% | 50 mm/hr |
| Arctic | -60-25°C | 40-90% | 10 mm/hr |
| Arid | -10-55°C | 5-70% | 25 mm/hr |

---

## Implementation

### Configuration File

The validation thresholds are implemented in:
```
backend/server/config/physics-validation-thresholds.js
```

### API Usage

```javascript
const thresholds = require('./physics-validation-thresholds');

// Get validation profile
const profile = thresholds.getValidationProfile('operational');

// Get climate-adjusted thresholds
const adjusted = thresholds.getClimateAdjustedThresholds('tropical', 'summer');

// Get altitude-adjusted thresholds
const altitudeAdjusted = thresholds.getAltitudeAdjustedThresholds(1500);
```

### Profile Selection by Use Case

| Use Case | Recommended Profile |
|----------|---------------------|
| Weather Monitoring | `operational` |
| Climate Research | `climate` |
| Aviation Services | `aviation` |
| Real-time Alerts | `realtime` |
| Scientific Studies | `research` |

---

## References

1. **WMO CIMO Guide** (WMO-No. 8): Guide to Meteorological Instruments and Methods of Observation
2. **ISO 2533:1975**: Standard Atmosphere
3. **NOAA Technical Memorandum NWS WRHQ 11-04**: Observation Standards
4. **AMS Glossary of Meteorology**: Measurement definitions and standards
5. **WMO Global Climate Observing System (GCOS)**: Climate monitoring requirements

---

*Last Updated: January 2026*
*Configuration Version: 1.0.0*
