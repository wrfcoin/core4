const express = require('express');
const cors = require('cors');

const app = express();
const PORT = process.env.PORT || 3020;

app.use(cors());
app.use(express.json());

// Mock emission factors database
const emissionFactors = {
  electricity_grid_us: 0.4531, // kg CO2e per kWh
  natural_gas: 0.2014,
  diesel_fuel: 2.676,
  weather_station_operation: 0.05,
  transport_car_gasoline: 0.21, // kg CO2e per km
  transport_flight_domestic: 0.255, // kg CO2e per km
};

// Mock carbon projects database
const carbonProjects = {
  'PROJ-001': {
    id: 'PROJ-001',
    name: 'Weather Station Network Expansion',
    type: 'monitoring_infrastructure',
    methodology: 'vm0012',
    vintage: 2024,
    location: { country: 'US', region: 'California' },
    creditsIssued: 1000,
    creditsRetired: 250,
    creditsAvailable: 750,
    pricePerCredit: 15.5,
    verificationStatus: 'verified',
    additionalityProof: true,
  },
  'PROJ-002': {
    id: 'PROJ-002',
    name: 'Renewable Energy Weather Monitoring',
    type: 'renewable_energy',
    methodology: 'vm0007',
    vintage: 2024,
    location: { country: 'US', region: 'Texas' },
    creditsIssued: 500,
    creditsRetired: 100,
    creditsAvailable: 400,
    pricePerCredit: 12.75,
    verificationStatus: 'verified',
    additionalityProof: true,
  },
};

// Health check endpoint
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    service: 'carbon-oracle-mock',
    timestamp: new Date().toISOString(),
  });
});

// Calculate carbon footprint
app.post('/api/carbon/calculate', (req, res) => {
  const {
    activityType,
    measurements,
    location,
    duration = 1,
    metadata = {},
  } = req.body;

  if (!activityType || !measurements) {
    return res.status(400).json({
      success: false,
      error: 'Missing required fields: activityType and measurements',
      timestamp: new Date().toISOString(),
    });
  }

  let totalCO2e = 0;
  const usedFactors = {};
  let uncertainty = 5;

  try {
    switch (activityType) {
      case 'weather_monitoring':
        const operationalHours = duration;
        const basePowerConsumption = 0.5; // kW per hour

        let powerAdjustment = 1.0;
        if (measurements.temperature && measurements.temperature < -10) {
          powerAdjustment += 0.2;
        }
        if (measurements.windSpeed && measurements.windSpeed > 15) {
          powerAdjustment += 0.1;
        }

        const totalPower =
          basePowerConsumption * operationalHours * powerAdjustment;
        const emissionFactor = emissionFactors['weather_station_operation'];
        totalCO2e = totalPower * emissionFactor;
        usedFactors['weather_station_operation'] = emissionFactor;
        break;

      case 'energy':
        if (measurements.electricity_kwh) {
          const factor = emissionFactors['electricity_grid_us'];
          totalCO2e += measurements.electricity_kwh * factor;
          usedFactors['electricity_grid_us'] = factor;
        }
        if (measurements.natural_gas_kwh) {
          const factor = emissionFactors['natural_gas'];
          totalCO2e += measurements.natural_gas_kwh * factor;
          usedFactors['natural_gas'] = factor;
        }
        uncertainty = 10;
        break;

      case 'transport':
        if (measurements.distance_km && measurements.fuel_type) {
          const factorKey = `transport_${measurements.fuel_type}`;
          const factor = emissionFactors[factorKey];
          if (factor) {
            totalCO2e = measurements.distance_km * factor;
            usedFactors[factorKey] = factor;
          }
        }
        uncertainty = 15;
        break;

      default:
        return res.status(400).json({
          success: false,
          error: `Unsupported activity type: ${activityType}`,
          timestamp: new Date().toISOString(),
        });
    }

    // Apply location-specific adjustments
    if (location && location.country) {
      const locationFactors = {
        US: 1.0,
        DE: 0.8, // Germany has cleaner electricity
        CN: 1.3, // China has dirtier electricity
        NO: 0.2, // Norway has very clean electricity
      };
      const locationFactor = locationFactors[location.country] || 1.0;
      totalCO2e *= locationFactor;
      usedFactors['location_adjustment'] = locationFactor;
    }

    const result = {
      co2Equivalent: Number(totalCO2e.toFixed(4)),
      calculationMethod: `${activityType}_ipcc_2021`,
      emissionFactors: usedFactors,
      uncertainty,
      source: 'WRFCoin Carbon Calculator v2.1',
      timestamp: new Date().toISOString(),
    };

    res.json({
      success: true,
      data: result,
      timestamp: new Date().toISOString(),
    });
  } catch (error) {
    console.error('Carbon calculation error:', error);
    res.status(500).json({
      success: false,
      error: 'Carbon calculation failed',
      details: error.message,
      timestamp: new Date().toISOString(),
    });
  }
});

// Calculate carbon credits earned
app.post('/api/carbon/credits/calculate', (req, res) => {
  const { footprint, activityType, metadata = {} } = req.body;

  if (!footprint || !activityType) {
    return res.status(400).json({
      success: false,
      error: 'Missing required fields: footprint and activityType',
      timestamp: new Date().toISOString(),
    });
  }

  let creditsEarned = 0;
  let creditType = 'voluntary';
  let methodology = '';
  let additionalityProof = false;

  switch (activityType) {
    case 'weather_monitoring':
      creditsEarned = footprint.co2Equivalent * 0.1; // 10% credit for monitoring
      creditType = 'adaptation';
      methodology = 'climate_monitoring_vm0012';
      additionalityProof = true;
      break;

    case 'renewable_energy':
      creditsEarned = footprint.co2Equivalent * 0.95; // 95% credit for renewable energy
      creditType = 'mitigation';
      methodology = 'renewable_energy_vm0007';
      additionalityProof = true;
      break;

    case 'energy_efficiency':
      creditsEarned = footprint.co2Equivalent * 0.8; // 80% credit for efficiency
      creditType = 'mitigation';
      methodology = 'energy_efficiency_vm0011';
      additionalityProof = true;
      break;

    default:
      creditsEarned = 0;
      creditType = 'none';
      methodology = 'not_applicable';
      additionalityProof = false;
  }

  const result = {
    creditsEarned: Number(creditsEarned.toFixed(6)),
    creditType,
    methodology,
    additionalityProof,
    timestamp: new Date().toISOString(),
  };

  res.json({
    success: true,
    data: result,
    timestamp: new Date().toISOString(),
  });
});

// Get carbon credit projects
app.get('/api/carbon/projects', (req, res) => {
  const { vintage, methodology, country, minPrice, maxPrice } = req.query;
  let projects = Object.values(carbonProjects);

  // Apply filters
  if (vintage) {
    projects = projects.filter(p => p.vintage === parseInt(vintage));
  }
  if (methodology) {
    projects = projects.filter(p => p.methodology === methodology);
  }
  if (country) {
    projects = projects.filter(p => p.location.country === country);
  }
  if (minPrice) {
    projects = projects.filter(p => p.pricePerCredit >= parseFloat(minPrice));
  }
  if (maxPrice) {
    projects = projects.filter(p => p.pricePerCredit <= parseFloat(maxPrice));
  }

  res.json({
    success: true,
    data: projects,
    count: projects.length,
    timestamp: new Date().toISOString(),
  });
});

// Get specific carbon project
app.get('/api/carbon/projects/:projectId', (req, res) => {
  const { projectId } = req.params;
  const project = carbonProjects[projectId];

  if (!project) {
    return res.status(404).json({
      success: false,
      error: 'Project not found',
      timestamp: new Date().toISOString(),
    });
  }

  res.json({
    success: true,
    data: project,
    timestamp: new Date().toISOString(),
  });
});

// Purchase carbon credits
app.post('/api/carbon/credits/purchase', (req, res) => {
  const { projectId, quantity, buyerAddress } = req.body;

  if (!projectId || !quantity || !buyerAddress) {
    return res.status(400).json({
      success: false,
      error: 'Missing required fields: projectId, quantity, buyerAddress',
      timestamp: new Date().toISOString(),
    });
  }

  const project = carbonProjects[projectId];
  if (!project) {
    return res.status(404).json({
      success: false,
      error: 'Project not found',
      timestamp: new Date().toISOString(),
    });
  }

  if (quantity > project.creditsAvailable) {
    return res.status(400).json({
      success: false,
      error: 'Insufficient credits available',
      available: project.creditsAvailable,
      requested: quantity,
      timestamp: new Date().toISOString(),
    });
  }

  // Generate mock transaction
  const transactionId = `tx_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
  const totalCost = quantity * project.pricePerCredit;

  // Update project availability (in real system, this would be database)
  project.creditsAvailable -= quantity;
  project.creditsRetired += quantity;

  const result = {
    transactionId,
    projectId,
    quantity,
    pricePerCredit: project.pricePerCredit,
    totalCost,
    buyerAddress,
    status: 'completed',
    carbonFootprintOffset: quantity, // 1 credit = 1 tonne CO2e
    certificateUrl: `https://registry.wrfcoin.com/certificates/${transactionId}`,
    timestamp: new Date().toISOString(),
  };

  res.json({
    success: true,
    data: result,
    timestamp: new Date().toISOString(),
  });
});

// Get emission factors
app.get('/api/carbon/emission-factors', (req, res) => {
  const { category } = req.query;
  let factors = emissionFactors;

  if (category) {
    factors = Object.fromEntries(
      Object.entries(emissionFactors).filter(([key]) =>
        key.startsWith(category)
      )
    );
  }

  res.json({
    success: true,
    data: factors,
    count: Object.keys(factors).length,
    timestamp: new Date().toISOString(),
  });
});

// Error handling
app.use((req, res) => {
  res.status(404).json({
    success: false,
    error: `Endpoint ${req.path} not found`,
    timestamp: new Date().toISOString(),
  });
});

app.use((error, req, res, next) => {
  console.error('Carbon Oracle Mock Error:', error);
  res.status(500).json({
    success: false,
    error: 'Internal server error',
    timestamp: new Date().toISOString(),
  });
});

app.listen(PORT, '0.0.0.0', () => {
  console.log(`Carbon Oracle Mock running on port ${PORT}`);
  console.log(`Health check: http://localhost:${PORT}/health`);
});
