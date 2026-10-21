const axios = require('axios');
const { Client } = require('pg');
const redis = require('redis');
const { v4: uuidv4 } = require('uuid');
const moment = require('moment');
const { faker } = require('@faker-js/faker');

console.log('🌱 WRFCoin Offline Development Data Seeder Starting...');

// Configuration from environment
const config = {
  database: {
    url:
      process.env.DATABASE_URL ||
      'postgresql://postgres:devpass@postgres-offline:5432/wrfcoin_dev',
  },
  redis: {
    url: process.env.REDIS_URL || 'redis://redis-offline:6379',
  },
  services: {
    api: process.env.API_URL || 'http://api-server-offline:4001',
    blockchain:
      process.env.BLOCKCHAIN_RPC_URL || 'http://blockchain-rpc-offline:8545',
    weather:
      process.env.WEATHER_API_URL || 'http://weather-oracle-offline:3010',
    carbon: process.env.CARBON_API_URL || 'http://carbon-oracle-offline:3020',
  },
  seed: {
    users: parseInt(process.env.SEED_USERS) || 50,
    weatherStations: parseInt(process.env.SEED_WEATHER_STATIONS) || 100,
    carbonProjects: parseInt(process.env.SEED_CARBON_PROJECTS) || 25,
    transactions: parseInt(process.env.SEED_TRANSACTIONS) || 1000,
    nftCollections: parseInt(process.env.SEED_NFT_COLLECTIONS) || 10,
  },
};

let dbClient;
let redisClient;

// Initialize connections
async function initializeConnections() {
  console.log('🔌 Connecting to services...');

  try {
    // PostgreSQL connection
    dbClient = new Client({ connectionString: config.database.url });
    await dbClient.connect();
    console.log('✅ Connected to PostgreSQL');

    // Redis connection
    redisClient = redis.createClient({ url: config.redis.url });
    await redisClient.connect();
    console.log('✅ Connected to Redis');

    return true;
  } catch (error) {
    console.error('❌ Connection failed:', error.message);
    return false;
  }
}

// Wait for services to be ready
async function waitForServices() {
  console.log('⏳ Waiting for services to be ready...');

  const maxRetries = 30;
  const retryDelay = 5000; // 5 seconds

  for (let retry = 0; retry < maxRetries; retry++) {
    try {
      // Check API server
      await axios.get(`${config.services.api}/health`, { timeout: 5000 });
      console.log('✅ API server is ready');

      // Check blockchain RPC
      await axios.get(`${config.services.blockchain}/health`, {
        timeout: 5000,
      });
      console.log('✅ Blockchain RPC is ready');

      // Check weather oracle
      await axios.get(`${config.services.weather}/health`, { timeout: 5000 });
      console.log('✅ Weather Oracle is ready');

      // Check carbon oracle
      await axios.get(`${config.services.carbon}/health`, { timeout: 5000 });
      console.log('✅ Carbon Oracle is ready');

      return true;
    } catch (error) {
      console.log(
        `⏳ Retry ${retry + 1}/${maxRetries}: Services not ready yet, waiting...`
      );
      await new Promise(resolve => setTimeout(resolve, retryDelay));
    }
  }

  console.error('❌ Services failed to become ready within timeout period');
  return false;
}

// Create database schema if not exists
async function ensureSchema() {
  console.log('🏗️  Ensuring database schema...');

  try {
    // Users table
    await dbClient.query(`
      CREATE TABLE IF NOT EXISTS users (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        username VARCHAR(255) UNIQUE NOT NULL,
        email VARCHAR(255) UNIQUE NOT NULL,
        wallet_address VARCHAR(42) UNIQUE NOT NULL,
        profile_data JSONB DEFAULT '{}',
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      );
    `);

    // Weather stations table
    await dbClient.query(`
      CREATE TABLE IF NOT EXISTS weather_stations (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        station_id VARCHAR(255) UNIQUE NOT NULL,
        name VARCHAR(255) NOT NULL,
        location JSONB NOT NULL,
        owner_id UUID REFERENCES users(id),
        metadata JSONB DEFAULT '{}',
        status VARCHAR(50) DEFAULT 'active',
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      );
    `);

    // Weather data table
    await dbClient.query(`
      CREATE TABLE IF NOT EXISTS weather_data (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        station_id UUID REFERENCES weather_stations(id),
        temperature DECIMAL(5,2),
        humidity DECIMAL(5,2),
        pressure DECIMAL(7,2),
        wind_speed DECIMAL(5,2),
        wind_direction INTEGER,
        precipitation DECIMAL(6,2),
        metadata JSONB DEFAULT '{}',
        recorded_at TIMESTAMP NOT NULL,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      );
    `);

    // Carbon projects table
    await dbClient.query(`
      CREATE TABLE IF NOT EXISTS carbon_projects (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        project_id VARCHAR(255) UNIQUE NOT NULL,
        name VARCHAR(255) NOT NULL,
        description TEXT,
        project_type VARCHAR(100) NOT NULL,
        location JSONB NOT NULL,
        owner_id UUID REFERENCES users(id),
        credits_issued INTEGER DEFAULT 0,
        credits_retired INTEGER DEFAULT 0,
        verification_status VARCHAR(50) DEFAULT 'pending',
        metadata JSONB DEFAULT '{}',
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      );
    `);

    // Transactions table
    await dbClient.query(`
      CREATE TABLE IF NOT EXISTS transactions (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        tx_hash VARCHAR(66) UNIQUE NOT NULL,
        from_address VARCHAR(42) NOT NULL,
        to_address VARCHAR(42) NOT NULL,
        value DECIMAL(20,8) NOT NULL,
        gas_used INTEGER,
        gas_price DECIMAL(20,8),
        status VARCHAR(50) DEFAULT 'pending',
        block_number INTEGER,
        transaction_type VARCHAR(100) NOT NULL,
        metadata JSONB DEFAULT '{}',
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      );
    `);

    // NFT collections table
    await dbClient.query(`
      CREATE TABLE IF NOT EXISTS nft_collections (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        collection_id VARCHAR(255) UNIQUE NOT NULL,
        name VARCHAR(255) NOT NULL,
        description TEXT,
        creator_id UUID REFERENCES users(id),
        contract_address VARCHAR(42),
        total_supply INTEGER DEFAULT 0,
        metadata JSONB DEFAULT '{}',
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      );
    `);

    // NFTs table
    await dbClient.query(`
      CREATE TABLE IF NOT EXISTS nfts (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        token_id INTEGER NOT NULL,
        collection_id UUID REFERENCES nft_collections(id),
        owner_id UUID REFERENCES users(id),
        name VARCHAR(255) NOT NULL,
        description TEXT,
        image_url TEXT,
        attributes JSONB DEFAULT '{}',
        metadata JSONB DEFAULT '{}',
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      );
    `);

    console.log('✅ Database schema ensured');
    return true;
  } catch (error) {
    console.error('❌ Schema creation failed:', error.message);
    return false;
  }
}

// Seed users
async function seedUsers() {
  console.log(`👥 Seeding ${config.seed.users} users...`);

  const users = [];
  for (let i = 0; i < config.seed.users; i++) {
    const user = {
      id: uuidv4(),
      username: faker.internet.userName(),
      email: faker.internet.email(),
      wallet_address: `0x${faker.string.hexadecimal({ length: 40, prefix: '' })}`,
      profile_data: {
        firstName: faker.person.firstName(),
        lastName: faker.person.lastName(),
        bio: faker.lorem.sentence(),
        avatar: faker.image.avatar(),
        location: `${faker.location.city()}, ${faker.location.country()}`,
        joined: faker.date.past({ years: 2 }),
        reputation: faker.number.int({ min: 0, max: 1000 }),
        badges: faker.helpers.arrayElements(
          [
            'early_adopter',
            'weather_contributor',
            'carbon_advocate',
            'data_validator',
            'nft_creator',
          ],
          { min: 0, max: 3 }
        ),
      },
    };
    users.push(user);
  }

  try {
    for (const user of users) {
      await dbClient.query(
        `
        INSERT INTO users (id, username, email, wallet_address, profile_data)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (username) DO NOTHING
      `,
        [
          user.id,
          user.username,
          user.email,
          user.wallet_address,
          JSON.stringify(user.profile_data),
        ]
      );
    }

    // Cache user IDs in Redis for quick access
    const userIds = users.map(u => u.id);
    await redisClient.set('seed:user_ids', JSON.stringify(userIds), {
      EX: 3600,
    });

    console.log('✅ Users seeded');
    return users;
  } catch (error) {
    console.error('❌ User seeding failed:', error.message);
    return [];
  }
}

// Seed weather stations
async function seedWeatherStations(users) {
  console.log(`🌡️  Seeding ${config.seed.weatherStations} weather stations...`);

  const stations = [];
  for (let i = 0; i < config.seed.weatherStations; i++) {
    const owner = faker.helpers.arrayElement(users);
    const lat = faker.location.latitude({ min: -60, max: 70 });
    const lng = faker.location.longitude();

    const station = {
      id: uuidv4(),
      station_id: `WS${faker.string.alphanumeric(8).toUpperCase()}`,
      name: `${faker.location.city()} Weather Station`,
      location: {
        latitude: lat,
        longitude: lng,
        altitude: faker.number.int({ min: 0, max: 4000 }),
        country: faker.location.country(),
        region: faker.location.state(),
        city: faker.location.city(),
      },
      owner_id: owner.id,
      metadata: {
        installation_date: faker.date.past({ years: 5 }),
        station_type: faker.helpers.arrayElement([
          'automatic',
          'manual',
          'hybrid',
        ]),
        sensors: faker.helpers.arrayElements(
          [
            'temperature',
            'humidity',
            'pressure',
            'wind',
            'precipitation',
            'solar_radiation',
            'uv_index',
            'air_quality',
          ],
          { min: 3, max: 8 }
        ),
        manufacturer: faker.company.name(),
        model: faker.string.alphanumeric(10).toUpperCase(),
        calibration_date: faker.date.recent({ days: 90 }),
      },
      status: faker.helpers.weightedArrayElement([
        { weight: 80, value: 'active' },
        { weight: 15, value: 'maintenance' },
        { weight: 5, value: 'offline' },
      ]),
    };
    stations.push(station);
  }

  try {
    for (const station of stations) {
      await dbClient.query(
        `
        INSERT INTO weather_stations (id, station_id, name, location, owner_id, metadata, status)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (station_id) DO NOTHING
      `,
        [
          station.id,
          station.station_id,
          station.name,
          JSON.stringify(station.location),
          station.owner_id,
          JSON.stringify(station.metadata),
          station.status,
        ]
      );
    }

    // Cache station IDs in Redis
    const stationIds = stations.map(s => s.id);
    await redisClient.set('seed:station_ids', JSON.stringify(stationIds), {
      EX: 3600,
    });

    console.log('✅ Weather stations seeded');
    return stations;
  } catch (error) {
    console.error('❌ Weather station seeding failed:', error.message);
    return [];
  }
}

// Seed weather data
async function seedWeatherData(stations) {
  console.log('🌦️  Seeding weather data (last 30 days)...');

  const dataPoints = [];
  const endDate = moment();
  const startDate = moment().subtract(30, 'days');

  for (const station of stations.slice(0, 20)) {
    // Limit to first 20 stations for performance
    let current = moment(startDate);

    while (current.isBefore(endDate)) {
      // Generate 4 data points per day (every 6 hours)
      for (let hour of [0, 6, 12, 18]) {
        const recordTime = moment(current).hour(hour);

        const dataPoint = {
          id: uuidv4(),
          station_id: station.id,
          temperature: faker.number.float({
            min: -20,
            max: 40,
            fractionDigits: 2,
          }),
          humidity: faker.number.float({
            min: 10,
            max: 100,
            fractionDigits: 2,
          }),
          pressure: faker.number.float({
            min: 950,
            max: 1050,
            fractionDigits: 2,
          }),
          wind_speed: faker.number.float({
            min: 0,
            max: 30,
            fractionDigits: 2,
          }),
          wind_direction: faker.number.int({ min: 0, max: 359 }),
          precipitation: faker.number.float({
            min: 0,
            max: 50,
            fractionDigits: 2,
          }),
          recorded_at: recordTime.toISOString(),
          metadata: {
            data_quality: faker.helpers.weightedArrayElement([
              { weight: 85, value: 'good' },
              { weight: 10, value: 'fair' },
              { weight: 5, value: 'poor' },
            ]),
            sensor_readings: {
              battery_level: faker.number.float({
                min: 0.2,
                max: 1.0,
                fractionDigits: 2,
              }),
              signal_strength: faker.number.int({ min: 1, max: 5 }),
            },
          },
        };
        dataPoints.push(dataPoint);
      }

      current.add(1, 'day');
    }
  }

  try {
    // Insert in batches for performance
    const batchSize = 100;
    for (let i = 0; i < dataPoints.length; i += batchSize) {
      const batch = dataPoints.slice(i, i + batchSize);

      for (const dataPoint of batch) {
        await dbClient.query(
          `
          INSERT INTO weather_data (id, station_id, temperature, humidity, pressure, 
                                  wind_speed, wind_direction, precipitation, metadata, recorded_at)
          VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        `,
          [
            dataPoint.id,
            dataPoint.station_id,
            dataPoint.temperature,
            dataPoint.humidity,
            dataPoint.pressure,
            dataPoint.wind_speed,
            dataPoint.wind_direction,
            dataPoint.precipitation,
            JSON.stringify(dataPoint.metadata),
            dataPoint.recorded_at,
          ]
        );
      }
    }

    console.log(`✅ Weather data seeded (${dataPoints.length} data points)`);
    return dataPoints.length;
  } catch (error) {
    console.error('❌ Weather data seeding failed:', error.message);
    return 0;
  }
}

// Seed carbon projects
async function seedCarbonProjects(users) {
  console.log(`🌱 Seeding ${config.seed.carbonProjects} carbon projects...`);

  const projects = [];
  const projectTypes = [
    'reforestation',
    'renewable_energy',
    'methane_capture',
    'energy_efficiency',
    'carbon_sequestration',
    'waste_management',
  ];

  for (let i = 0; i < config.seed.carbonProjects; i++) {
    const owner = faker.helpers.arrayElement(users);

    const project = {
      id: uuidv4(),
      project_id: `CP${faker.string.alphanumeric(8).toUpperCase()}`,
      name: `${faker.company.buzzNoun()} ${faker.helpers.arrayElement(projectTypes).replace('_', ' ')} Project`,
      description: faker.lorem.paragraphs(2),
      project_type: faker.helpers.arrayElement(projectTypes),
      location: {
        latitude: faker.location.latitude(),
        longitude: faker.location.longitude(),
        country: faker.location.country(),
        region: faker.location.state(),
      },
      owner_id: owner.id,
      credits_issued: faker.number.int({ min: 1000, max: 100000 }),
      credits_retired: faker.number.int({ min: 0, max: 50000 }),
      verification_status: faker.helpers.weightedArrayElement([
        { weight: 60, value: 'verified' },
        { weight: 25, value: 'pending' },
        { weight: 10, value: 'in_review' },
        { weight: 5, value: 'rejected' },
      ]),
      metadata: {
        start_date: faker.date.past({ years: 3 }),
        end_date: faker.date.future({ years: 5 }),
        methodology: faker.helpers.arrayElement([
          'VCS',
          'CDM',
          'Gold Standard',
          'CAR',
          'ACR',
        ]),
        vintage_year: faker.date.recent({ days: 365 }).getFullYear(),
        verification_body: faker.company.name(),
        sdg_impacts: faker.helpers.arrayElements(
          ['SDG 1', 'SDG 3', 'SDG 6', 'SDG 7', 'SDG 13', 'SDG 15'],
          { min: 1, max: 4 }
        ),
      },
    };
    projects.push(project);
  }

  try {
    for (const project of projects) {
      await dbClient.query(
        `
        INSERT INTO carbon_projects (id, project_id, name, description, project_type, location, 
                                   owner_id, credits_issued, credits_retired, verification_status, metadata)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (project_id) DO NOTHING
      `,
        [
          project.id,
          project.project_id,
          project.name,
          project.description,
          project.project_type,
          JSON.stringify(project.location),
          project.owner_id,
          project.credits_issued,
          project.credits_retired,
          project.verification_status,
          JSON.stringify(project.metadata),
        ]
      );
    }

    console.log('✅ Carbon projects seeded');
    return projects;
  } catch (error) {
    console.error('❌ Carbon project seeding failed:', error.message);
    return [];
  }
}

// Cleanup and exit
async function cleanup() {
  console.log('🧹 Cleaning up connections...');

  try {
    if (dbClient) {
      await dbClient.end();
      console.log('✅ Database connection closed');
    }

    if (redisClient) {
      await redisClient.disconnect();
      console.log('✅ Redis connection closed');
    }
  } catch (error) {
    console.error('⚠️  Cleanup warning:', error.message);
  }
}

// Main seeding process
async function main() {
  try {
    console.log('🚀 Starting WRFCoin Development Data Seeding...');
    console.log('Configuration:', {
      users: config.seed.users,
      weatherStations: config.seed.weatherStations,
      carbonProjects: config.seed.carbonProjects,
      transactions: config.seed.transactions,
      nftCollections: config.seed.nftCollections,
    });

    // Wait for services
    if (!(await waitForServices())) {
      process.exit(1);
    }

    // Initialize connections
    if (!(await initializeConnections())) {
      process.exit(1);
    }

    // Ensure database schema
    if (!(await ensureSchema())) {
      process.exit(1);
    }

    // Seed data in order
    console.log('📊 Starting data seeding process...');

    const users = await seedUsers();
    if (users.length === 0) {
      console.error('❌ Failed to seed users, aborting');
      process.exit(1);
    }

    const stations = await seedWeatherStations(users);
    if (stations.length === 0) {
      console.error('❌ Failed to seed weather stations, aborting');
      process.exit(1);
    }

    await seedWeatherData(stations);
    await seedCarbonProjects(users);

    // Cache seed metadata in Redis
    await redisClient.set(
      'seed:metadata',
      JSON.stringify({
        seeded_at: new Date().toISOString(),
        users_count: users.length,
        stations_count: stations.length,
        environment: 'offline_development',
      }),
      { EX: 86400 }
    ); // 24 hours

    console.log('🎉 Data seeding completed successfully!');
    console.log('📈 Summary:');
    console.log(`   - Users: ${users.length}`);
    console.log(`   - Weather Stations: ${stations.length}`);
    console.log(`   - Carbon Projects: ${config.seed.carbonProjects}`);
    console.log(`   - Weather Data Points: Generated for last 30 days`);

    await cleanup();
    process.exit(0);
  } catch (error) {
    console.error('💥 Seeding process failed:', error);
    await cleanup();
    process.exit(1);
  }
}

// Handle graceful shutdown
process.on('SIGTERM', async () => {
  console.log('🛑 Received SIGTERM, shutting down gracefully...');
  await cleanup();
  process.exit(0);
});

process.on('SIGINT', async () => {
  console.log('🛑 Received SIGINT, shutting down gracefully...');
  await cleanup();
  process.exit(0);
});

// Start the seeding process
main();
