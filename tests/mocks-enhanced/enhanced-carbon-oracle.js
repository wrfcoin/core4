#!/usr/bin/env node

/**
 * Enhanced Carbon Credit Oracle Mock Server
 * Provides comprehensive carbon credit trading, verification, and offset APIs
 * Supports real-time pricing, project tracking, and advanced carbon accounting
 */

const express = require('express');
const cors = require('cors');
const { v4: uuidv4 } = require('uuid');
const moment = require('moment');

const app = express();
const PORT = process.env.PORT || 3020;
const TRACKING_PORT = process.env.TRACKING_PORT || 3021;

// Configuration
const config = {
  projectCount: parseInt(process.env.PROJECT_COUNT) || 25,
  emissionFactorSources: parseInt(process.env.EMISSION_FACTOR_SOURCES) || 10,
  enableHistoricalData: process.env.ENABLE_HISTORICAL_DATA === 'true',
  carbonPriceVolatility: process.env.CARBON_PRICE_VOLATILITY || 'medium',
  enableOffsetVerification: process.env.ENABLE_OFFSET_VERIFICATION === 'true',
};

// Carbon project types and their characteristics
const projectTypes = [
  {
    type: 'renewable_energy',
    name: 'Renewable Energy',
    avgPrice: 12,
    volatility: 0.15,
    credibility: 0.95,
  },
  {
    type: 'reforestation',
    name: 'Reforestation & Afforestation',
    avgPrice: 8,
    volatility: 0.25,
    credibility: 0.85,
  },
  {
    type: 'energy_efficiency',
    name: 'Energy Efficiency',
    avgPrice: 10,
    volatility: 0.1,
    credibility: 0.9,
  },
  {
    type: 'methane_capture',
    name: 'Methane Capture',
    avgPrice: 15,
    volatility: 0.2,
    credibility: 0.88,
  },
  {
    type: 'direct_air_capture',
    name: 'Direct Air Capture',
    avgPrice: 45,
    volatility: 0.4,
    credibility: 0.95,
  },
  {
    type: 'blue_carbon',
    name: 'Blue Carbon (Marine)',
    avgPrice: 18,
    volatility: 0.3,
    credibility: 0.8,
  },
  {
    type: 'soil_carbon',
    name: 'Soil Carbon Sequestration',
    avgPrice: 6,
    volatility: 0.35,
    credibility: 0.75,
  },
  {
    type: 'waste_management',
    name: 'Waste Management',
    avgPrice: 9,
    volatility: 0.15,
    credibility: 0.85,
  },
  {
    type: 'transport',
    name: 'Transportation',
    avgPrice: 11,
    volatility: 0.2,
    credibility: 0.88,
  },
  {
    type: 'industrial',
    name: 'Industrial Process',
    avgPrice: 13,
    volatility: 0.18,
    credibility: 0.92,
  },
];

// Carbon standards and registries
const carbonStandards = [
  {
    name: 'VCS',
    fullName: 'Verified Carbon Standard',
    reputation: 0.95,
    feePercent: 3,
  },
  {
    name: 'CDM',
    fullName: 'Clean Development Mechanism',
    reputation: 0.9,
    feePercent: 5,
  },
  { name: 'GS', fullName: 'Gold Standard', reputation: 0.98, feePercent: 4 },
  {
    name: 'ACR',
    fullName: 'American Carbon Registry',
    reputation: 0.88,
    feePercent: 3.5,
  },
  {
    name: 'CAR',
    fullName: 'Climate Action Reserve',
    reputation: 0.92,
    feePercent: 3,
  },
  {
    name: 'CCBS',
    fullName: 'Climate, Community & Biodiversity Standards',
    reputation: 0.85,
    feePercent: 4.5,
  },
  {
    name: 'REDD+',
    fullName: 'REDD+ Programme',
    reputation: 0.8,
    feePercent: 6,
  },
  {
    name: 'WRFChain',
    fullName: 'WRFCoin Blockchain Registry',
    reputation: 0.99,
    feePercent: 2,
  },
];

// Mock carbon projects
const carbonProjects = [];
const projectCredits = new Map(); // Available credits per project
const historicalPrices = new Map(); // Historical pricing data
const transactions = new Map(); // Transaction history
const verificationQueue = []; // Projects awaiting verification

// Emission factors database (kg CO2e per unit)
const emissionFactors = {
  electricity: {
    coal: 1.02,
    natural_gas: 0.49,
    oil: 0.73,
    solar: 0.046,
    wind: 0.026,
    hydro: 0.026,
    nuclear: 0.029,
    geothermal: 0.045,
    biomass: 0.18,
    grid_average_us: 0.85,
    grid_average_eu: 0.62,
    grid_average_china: 1.08,
    grid_average_india: 0.95,
  },
  transportation: {
    car_gasoline: 0.251,
    car_diesel: 0.265,
    car_electric: 0.089,
    bus: 0.089,
    train_diesel: 0.084,
    train_electric: 0.048,
    airplane_domestic: 0.255,
    airplane_international: 0.285,
    ship_cargo: 0.038,
    truck_heavy: 0.161,
    motorcycle: 0.103,
  },
  heating: {
    natural_gas: 0.203,
    propane: 0.215,
    heating_oil: 0.276,
    wood: 0.013,
    heat_pump: 0.089,
    district_heating: 0.156,
  },
  industrial: {
    cement: 0.927,
    steel: 1.95,
    aluminum: 1.69,
    paper: 0.89,
    chemicals: 1.45,
    textiles: 0.78,
    food_processing: 0.34,
  },
  agriculture: {
    beef: 21.3,
    pork: 3.8,
    chicken: 2.3,
    dairy: 1.37,
    rice: 1.15,
    wheat: 0.45,
    corn: 0.39,
    soybeans: 0.35,
  },
};

// Initialize mock carbon projects
function initializeProjects() {
  for (let i = 0; i < config.projectCount; i++) {
    const projectType =
      projectTypes[Math.floor(Math.random() * projectTypes.length)];
    const standard =
      carbonStandards[Math.floor(Math.random() * carbonStandards.length)];

    const project = {
      id: `CCProject-${String(i + 1).padStart(4, '0')}`,
      name: `${projectType.name} Project ${i + 1}`,
      type: projectType.type,
      standard: standard.name,
      standardFull: standard.fullName,
      location: {
        country: ['US', 'CA', 'BR', 'IN', 'CN', 'KE', 'ID', 'PE', 'AU', 'DE'][
          Math.floor(Math.random() * 10)
        ],
        region: `Region-${Math.floor(Math.random() * 20) + 1}`,
        coordinates: {
          lat: (Math.random() - 0.5) * 180,
          lng: (Math.random() - 0.5) * 360,
        },
      },
      developer: `Developer ${Math.floor(Math.random() * 50) + 1}`,
      status: ['active', 'pending_verification', 'verified', 'suspended'][
        Math.floor(Math.random() * 4)
      ],
      startDate: moment()
        .subtract(Math.floor(Math.random() * 365 * 3), 'days')
        .toISOString(),
      endDate: moment()
        .add(Math.floor(Math.random() * 365 * 10) + 365, 'days')
        .toISOString(),
      creditingPeriod: Math.floor(Math.random() * 20) + 5, // 5-25 years
      totalCreditsIssued: Math.floor(Math.random() * 500000) + 10000,
      creditsAvailable: 0, // Will be calculated
      vintageYear: 2024 - Math.floor(Math.random() * 5),
      methodology: `${standard.name}-${Math.floor(Math.random() * 100) + 1}`,
      additionality: Math.random() > 0.2, // 80% have additionality
      permanence:
        projectType.type === 'reforestation'
          ? Math.random() > 0.3
          : Math.random() > 0.1,
      verification: {
        status: ['verified', 'pending', 'failed'][
          Math.floor(Math.random() * 3)
        ],
        lastVerificationDate: moment()
          .subtract(Math.floor(Math.random() * 365), 'days')
          .toISOString(),
        nextVerificationDate: moment()
          .add(Math.floor(Math.random() * 365) + 180, 'days')
          .toISOString(),
        verifier: `Verification Body ${Math.floor(Math.random() * 10) + 1}`,
      },
      pricing: {
        currentPrice:
          projectType.avgPrice +
          (Math.random() - 0.5) * projectType.avgPrice * 0.4,
        currency: 'USD',
        pricePerTonCO2e: true,
      },
      sustainability: {
        sdgAlignment: Math.floor(Math.random() * 17) + 1, // SDG goals 1-17
        biodiversityImpact: Math.random() > 0.5,
        communityBenefits: Math.random() > 0.3,
        genderInclusive: Math.random() > 0.4,
      },
      documentation: {
        pdd: `https://registry.wrfcoin.com/projects/${i + 1}/pdd.pdf`,
        monitoringReport: `https://registry.wrfcoin.com/projects/${i + 1}/monitoring.pdf`,
        verificationReport: `https://registry.wrfcoin.com/projects/${i + 1}/verification.pdf`,
        images: [`https://registry.wrfcoin.com/projects/${i + 1}/image1.jpg`],
      },
      metadata: {
        created: moment()
          .subtract(Math.floor(Math.random() * 365 * 2), 'days')
          .toISOString(),
        lastUpdated: moment()
          .subtract(Math.floor(Math.random() * 30), 'days')
          .toISOString(),
        credibility: projectType.credibility + (Math.random() - 0.5) * 0.1,
        riskRating: ['low', 'medium', 'high'][Math.floor(Math.random() * 3)],
      },
    };

    // Calculate available credits
    project.creditsAvailable = Math.floor(
      project.totalCreditsIssued * (0.5 + Math.random() * 0.5)
    );

    carbonProjects.push(project);
    projectCredits.set(project.id, project.creditsAvailable);

    // Initialize historical prices
    if (config.enableHistoricalData) {
      const prices = [];
      let currentPrice = project.pricing.currentPrice;

      for (let days = 365; days > 0; days--) {
        const volatility = projectType.volatility;
        const priceChange = (Math.random() - 0.5) * volatility * currentPrice;
        currentPrice = Math.max(0.5, currentPrice + priceChange);

        prices.push({
          date: moment().subtract(days, 'days').toISOString(),
          price: parseFloat(currentPrice.toFixed(2)),
          volume: Math.floor(Math.random() * 1000) + 100,
        });
      }

      historicalPrices.set(project.id, prices);
    }
  }

  console.log(`Initialized ${config.projectCount} mock carbon projects`);
}

// Middleware
app.use(
  cors({
    origin: '*',
    methods: ['GET', 'POST', 'PUT', 'DELETE', 'OPTIONS'],
    allowedHeaders: ['Content-Type', 'Authorization', 'X-API-Key'],
  })
);
app.use(express.json());

// Health check endpoint
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    service: process.env.SERVICE_NAME || 'carbon-oracle-enhanced',
    timestamp: new Date().toISOString(),
    config: {
      projects: config.projectCount,
      emissionFactorSources: config.emissionFactorSources,
      historicalData: config.enableHistoricalData,
      priceVolatility: config.carbonPriceVolatility,
    },
    stats: {
      totalProjects: carbonProjects.length,
      activeProjects: carbonProjects.filter(p => p.status === 'active').length,
      verifiedProjects: carbonProjects.filter(
        p => p.verification.status === 'verified'
      ).length,
      totalCreditsAvailable: Array.from(projectCredits.values()).reduce(
        (sum, credits) => sum + credits,
        0
      ),
      transactions: transactions.size,
    },
  });
});

// List carbon projects
app.get('/api/carbon/projects', (req, res) => {
  const {
    type,
    standard,
    country,
    status,
    minPrice,
    maxPrice,
    vintageYear,
    verified,
    limit = 20,
    offset = 0,
    sortBy = 'name',
    sortOrder = 'asc',
  } = req.query;

  let projects = [...carbonProjects];

  // Apply filters
  if (type) projects = projects.filter(p => p.type === type);
  if (standard) projects = projects.filter(p => p.standard === standard);
  if (country) projects = projects.filter(p => p.location.country === country);
  if (status) projects = projects.filter(p => p.status === status);
  if (minPrice)
    projects = projects.filter(
      p => p.pricing.currentPrice >= parseFloat(minPrice)
    );
  if (maxPrice)
    projects = projects.filter(
      p => p.pricing.currentPrice <= parseFloat(maxPrice)
    );
  if (vintageYear)
    projects = projects.filter(p => p.vintageYear === parseInt(vintageYear));
  if (verified === 'true')
    projects = projects.filter(p => p.verification.status === 'verified');

  // Sort projects
  projects.sort((a, b) => {
    let aVal = a[sortBy];
    let bVal = b[sortBy];

    if (sortBy === 'price') {
      aVal = a.pricing.currentPrice;
      bVal = b.pricing.currentPrice;
    }

    if (typeof aVal === 'string') {
      return sortOrder === 'asc'
        ? aVal.localeCompare(bVal)
        : bVal.localeCompare(aVal);
    }

    return sortOrder === 'asc' ? aVal - bVal : bVal - aVal;
  });

  // Pagination
  const total = projects.length;
  projects = projects.slice(offset, offset + parseInt(limit));

  res.json({
    projects: projects.map(p => ({
      ...p,
      creditsAvailable: projectCredits.get(p.id),
    })),
    pagination: {
      total,
      limit: parseInt(limit),
      offset: parseInt(offset),
      hasMore: offset + projects.length < total,
    },
    filters: {
      availableTypes: [...new Set(carbonProjects.map(p => p.type))],
      availableStandards: [...new Set(carbonProjects.map(p => p.standard))],
      availableCountries: [
        ...new Set(carbonProjects.map(p => p.location.country)),
      ],
      priceRange: {
        min: Math.min(...carbonProjects.map(p => p.pricing.currentPrice)),
        max: Math.max(...carbonProjects.map(p => p.pricing.currentPrice)),
      },
    },
  });
});

// Get specific carbon project
app.get('/api/carbon/projects/:projectId', (req, res) => {
  const { projectId } = req.params;
  const project = carbonProjects.find(p => p.id === projectId);

  if (!project) {
    return res.status(404).json({
      error: 'Project not found',
      projectId,
    });
  }

  const creditsAvailable = projectCredits.get(projectId);
  const priceHistory = historicalPrices.get(projectId) || [];

  res.json({
    ...project,
    creditsAvailable,
    priceHistory: priceHistory.slice(-30), // Last 30 data points
    nearbyProjects: carbonProjects
      .filter(
        p =>
          p.id !== projectId && p.location.country === project.location.country
      )
      .slice(0, 5)
      .map(p => ({
        id: p.id,
        name: p.name,
        type: p.type,
        price: p.pricing.currentPrice,
      })),
  });
});

// Carbon emission factors
app.get('/api/carbon/emission-factors', (req, res) => {
  const { category, search } = req.query;

  let factors = { ...emissionFactors };

  if (category && factors[category]) {
    factors = { [category]: factors[category] };
  }

  if (search) {
    const filteredFactors = {};
    Object.keys(factors).forEach(cat => {
      const filtered = {};
      Object.keys(factors[cat]).forEach(key => {
        if (key.toLowerCase().includes(search.toLowerCase())) {
          filtered[key] = factors[cat][key];
        }
      });
      if (Object.keys(filtered).length > 0) {
        filteredFactors[cat] = filtered;
      }
    });
    factors = filteredFactors;
  }

  res.json({
    emissionFactors: factors,
    metadata: {
      units: 'kg CO2e per unit',
      source: 'WRFCoin Carbon Database v2.1',
      lastUpdated: '2024-01-15',
      categories: Object.keys(emissionFactors),
    },
  });
});

// Calculate carbon footprint
app.post('/api/carbon/calculate', (req, res) => {
  const { activities } = req.body;

  if (!activities || !Array.isArray(activities)) {
    return res.status(400).json({
      error: 'Activities array is required',
      example: {
        activities: [
          {
            type: 'electricity',
            fuel: 'grid_average_us',
            amount: 1000,
            unit: 'kWh',
          },
          {
            type: 'transportation',
            fuel: 'car_gasoline',
            amount: 500,
            unit: 'km',
          },
        ],
      },
    });
  }

  let totalEmissions = 0;
  const calculationBreakdown = [];
  const errors = [];

  activities.forEach((activity, index) => {
    const { type, fuel, amount, unit } = activity;

    if (!type || !fuel || amount === undefined) {
      errors.push({
        index,
        error: 'Missing required fields: type, fuel, amount',
        activity,
      });
      return;
    }

    const categoryFactors = emissionFactors[type];
    if (!categoryFactors) {
      errors.push({
        index,
        error: `Unknown activity type: ${type}`,
        availableTypes: Object.keys(emissionFactors),
      });
      return;
    }

    const emissionFactor = categoryFactors[fuel];
    if (emissionFactor === undefined) {
      errors.push({
        index,
        error: `Unknown fuel type: ${fuel} for category: ${type}`,
        availableFuels: Object.keys(categoryFactors),
      });
      return;
    }

    const emissions = amount * emissionFactor;
    totalEmissions += emissions;

    calculationBreakdown.push({
      index,
      type,
      fuel,
      amount,
      unit: unit || 'unit',
      emissionFactor,
      emissions: parseFloat(emissions.toFixed(3)),
      percentage: 0, // Will be calculated after total
    });
  });

  // Calculate percentages
  calculationBreakdown.forEach(item => {
    item.percentage = parseFloat(
      ((item.emissions / totalEmissions) * 100).toFixed(1)
    );
  });

  res.json({
    totalEmissions: parseFloat(totalEmissions.toFixed(3)),
    unit: 'kg CO2e',
    breakdown: calculationBreakdown,
    errors: errors.length > 0 ? errors : undefined,
    recommendations: generateOffsetRecommendations(totalEmissions),
  });
});

// Generate offset recommendations
function generateOffsetRecommendations(emissions) {
  const recommendedProjects = carbonProjects
    .filter(p => p.status === 'active' && p.verification.status === 'verified')
    .sort((a, b) => a.pricing.currentPrice - b.pricing.currentPrice)
    .slice(0, 3)
    .map(p => ({
      projectId: p.id,
      name: p.name,
      type: p.type,
      pricePerTon: p.pricing.currentPrice,
      totalCost: parseFloat(
        ((emissions / 1000) * p.pricing.currentPrice).toFixed(2)
      ),
      credibility: p.metadata.credibility,
    }));

  return {
    offsetAmount: parseFloat((emissions / 1000).toFixed(3)), // Convert to tons
    unit: 'tons CO2e',
    recommendedProjects,
    totalCostRange: {
      min: Math.min(...recommendedProjects.map(p => p.totalCost)),
      max: Math.max(...recommendedProjects.map(p => p.totalCost)),
    },
  };
}

// Purchase carbon credits
app.post('/api/carbon/credits/purchase', (req, res) => {
  const { projectId, quantity, buyerInfo } = req.body;

  if (!projectId || !quantity || quantity <= 0) {
    return res.status(400).json({
      error: 'Project ID and positive quantity are required',
    });
  }

  const project = carbonProjects.find(p => p.id === projectId);
  if (!project) {
    return res.status(404).json({
      error: 'Project not found',
      projectId,
    });
  }

  const availableCredits = projectCredits.get(projectId);
  if (quantity > availableCredits) {
    return res.status(400).json({
      error: 'Insufficient credits available',
      requested: quantity,
      available: availableCredits,
    });
  }

  // Create transaction
  const transaction = {
    id: uuidv4(),
    type: 'purchase',
    projectId,
    projectName: project.name,
    quantity,
    pricePerCredit: project.pricing.currentPrice,
    totalPrice: quantity * project.pricing.currentPrice,
    currency: 'USD',
    buyer: buyerInfo || { name: 'Anonymous Buyer' },
    timestamp: new Date().toISOString(),
    status: 'completed',
    serialNumbers: generateSerialNumbers(quantity),
    retirement: {
      retired: false,
      retirementReason: null,
      retirementDate: null,
    },
    verification: {
      transactionHash: generateTransactionHash(),
      blockNumber: Math.floor(Math.random() * 1000000) + 500000,
      verified: true,
    },
  };

  // Update available credits
  projectCredits.set(projectId, availableCredits - quantity);
  transactions.set(transaction.id, transaction);

  res.json({
    success: true,
    transaction,
    remainingCredits: projectCredits.get(projectId),
    certificateUrl: `https://registry.wrfcoin.com/certificates/${transaction.id}.pdf`,
  });
});

// Retire carbon credits
app.post('/api/carbon/credits/retire', (req, res) => {
  const { transactionId, retirementReason, onBehalfOf } = req.body;

  if (!transactionId) {
    return res.status(400).json({
      error: 'Transaction ID is required',
    });
  }

  const transaction = transactions.get(transactionId);
  if (!transaction) {
    return res.status(404).json({
      error: 'Transaction not found',
      transactionId,
    });
  }

  if (transaction.retirement.retired) {
    return res.status(400).json({
      error: 'Credits already retired',
      retirementDate: transaction.retirement.retirementDate,
    });
  }

  // Update transaction
  transaction.retirement = {
    retired: true,
    retirementReason: retirementReason || 'Voluntary retirement',
    retirementDate: new Date().toISOString(),
    onBehalfOf: onBehalfOf || transaction.buyer.name,
  };

  res.json({
    success: true,
    transaction,
    retirementCertificateUrl: `https://registry.wrfcoin.com/retirement/${transactionId}.pdf`,
  });
});

// Get carbon credit transactions
app.get('/api/carbon/transactions', (req, res) => {
  const { projectId, buyerId, limit = 20, offset = 0 } = req.query;

  let txs = Array.from(transactions.values());

  if (projectId) txs = txs.filter(tx => tx.projectId === projectId);
  if (buyerId) txs = txs.filter(tx => tx.buyer.id === buyerId);

  // Sort by timestamp (newest first)
  txs.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));

  const total = txs.length;
  txs = txs.slice(offset, offset + parseInt(limit));

  res.json({
    transactions: txs,
    pagination: {
      total,
      limit: parseInt(limit),
      offset: parseInt(offset),
      hasMore: offset + txs.length < total,
    },
  });
});

// Carbon market analytics
app.get('/api/carbon/market/analytics', (req, res) => {
  const { period = '30d' } = req.query;

  const periodDays = period === '7d' ? 7 : period === '30d' ? 30 : 365;
  const startDate = moment().subtract(periodDays, 'days');

  const analytics = {
    marketOverview: {
      totalProjects: carbonProjects.length,
      activeProjects: carbonProjects.filter(p => p.status === 'active').length,
      verifiedProjects: carbonProjects.filter(
        p => p.verification.status === 'verified'
      ).length,
      totalCreditsAvailable: Array.from(projectCredits.values()).reduce(
        (sum, credits) => sum + credits,
        0
      ),
      averagePrice: parseFloat(
        (
          carbonProjects.reduce((sum, p) => sum + p.pricing.currentPrice, 0) /
          carbonProjects.length
        ).toFixed(2)
      ),
    },
    priceAnalytics: {
      period: period,
      minPrice: Math.min(...carbonProjects.map(p => p.pricing.currentPrice)),
      maxPrice: Math.max(...carbonProjects.map(p => p.pricing.currentPrice)),
      priceByType: {},
    },
    volumeAnalytics: {
      totalVolume: Array.from(transactions.values())
        .filter(tx => moment(tx.timestamp).isAfter(startDate))
        .reduce((sum, tx) => sum + tx.quantity, 0),
      transactionCount: Array.from(transactions.values()).filter(tx =>
        moment(tx.timestamp).isAfter(startDate)
      ).length,
    },
    trends: {
      topProjects: carbonProjects
        .sort((a, b) => projectCredits.get(b.id) - projectCredits.get(a.id))
        .slice(0, 5)
        .map(p => ({
          id: p.id,
          name: p.name,
          type: p.type,
          creditsAvailable: projectCredits.get(p.id),
          price: p.pricing.currentPrice,
        })),
    },
  };

  // Calculate price by type
  projectTypes.forEach(type => {
    const typeProjects = carbonProjects.filter(p => p.type === type.type);
    if (typeProjects.length > 0) {
      analytics.priceAnalytics.priceByType[type.type] = {
        average: parseFloat(
          (
            typeProjects.reduce((sum, p) => sum + p.pricing.currentPrice, 0) /
            typeProjects.length
          ).toFixed(2)
        ),
        count: typeProjects.length,
        totalCredits: typeProjects.reduce(
          (sum, p) => sum + projectCredits.get(p.id),
          0
        ),
      };
    }
  });

  res.json(analytics);
});

// Utility functions
function generateSerialNumbers(quantity) {
  const numbers = [];
  for (let i = 0; i < quantity; i++) {
    numbers.push(`WRF${Math.random().toString(36).substr(2, 9).toUpperCase()}`);
  }
  return numbers;
}

function generateTransactionHash() {
  return (
    '0x' +
    Array.from({ length: 64 }, () =>
      Math.floor(Math.random() * 16).toString(16)
    ).join('')
  );
}

// Start tracking server (if different port)
if (TRACKING_PORT !== PORT) {
  const trackingApp = express();
  trackingApp.use(cors());
  trackingApp.use(express.json());

  // Carbon tracking API
  trackingApp.get('/api/track/:serialNumber', (req, res) => {
    const { serialNumber } = req.params;

    // Find transaction with this serial number
    const transaction = Array.from(transactions.values()).find(
      tx => tx.serialNumbers && tx.serialNumbers.includes(serialNumber)
    );

    if (!transaction) {
      return res.status(404).json({
        error: 'Credit not found',
        serialNumber,
      });
    }

    const project = carbonProjects.find(p => p.id === transaction.projectId);

    res.json({
      serialNumber,
      project: {
        id: project.id,
        name: project.name,
        type: project.type,
        location: project.location,
      },
      transaction: {
        id: transaction.id,
        purchaseDate: transaction.timestamp,
        quantity: 1, // Each serial number represents 1 credit
        retired: transaction.retirement.retired,
        retirementDate: transaction.retirement.retirementDate,
      },
      verification: transaction.verification,
    });
  });

  trackingApp.listen(TRACKING_PORT, '0.0.0.0', () => {
    console.log(`Carbon tracking server running on port ${TRACKING_PORT}`);
  });
}

// Initialize and start main server
initializeProjects();

app.listen(PORT, '0.0.0.0', () => {
  console.log(`Enhanced Carbon Oracle Mock Server running on port ${PORT}`);
  console.log(`Projects: ${config.projectCount}`);
  console.log(`Historical data: ${config.enableHistoricalData}`);
  console.log(`Price volatility: ${config.carbonPriceVolatility}`);
  console.log(`Offset verification: ${config.enableOffsetVerification}`);
});
