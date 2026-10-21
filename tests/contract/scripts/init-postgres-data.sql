-- Test data initialization for PostgreSQL mock
-- This script creates comprehensive test data for integration testing

-- Enable extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- Create test schemas
CREATE SCHEMA IF NOT EXISTS weather;
CREATE SCHEMA IF NOT EXISTS carbon;
CREATE SCHEMA IF NOT EXISTS blockchain;
CREATE SCHEMA IF NOT EXISTS users;

-- Weather stations table
CREATE TABLE IF NOT EXISTS weather.stations (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    station_id VARCHAR(50) UNIQUE NOT NULL,
    name VARCHAR(200) NOT NULL,
    latitude DECIMAL(10,6) NOT NULL,
    longitude DECIMAL(10,6) NOT NULL,
    elevation INTEGER DEFAULT 0,
    country_code VARCHAR(2),
    region VARCHAR(100),
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Weather data table
CREATE TABLE IF NOT EXISTS weather.readings (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    station_id VARCHAR(50) REFERENCES weather.stations(station_id),
    recorded_at TIMESTAMP NOT NULL,
    temperature DECIMAL(5,2),
    humidity INTEGER,
    pressure DECIMAL(7,2),
    wind_speed DECIMAL(5,2),
    wind_direction INTEGER,
    precipitation DECIMAL(7,2),
    visibility DECIMAL(6,2),
    conditions VARCHAR(50),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Carbon markets table
CREATE TABLE IF NOT EXISTS carbon.markets (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    market_id VARCHAR(20) UNIQUE NOT NULL,
    name VARCHAR(200) NOT NULL,
    type VARCHAR(50) NOT NULL, -- voluntary, compliance, premium
    base_price DECIMAL(10,2) NOT NULL,
    current_price DECIMAL(10,2) NOT NULL,
    currency VARCHAR(3) DEFAULT 'USD',
    unit VARCHAR(10) DEFAULT 'tCO2e',
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Carbon transactions table
CREATE TABLE IF NOT EXISTS carbon.transactions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    market_id VARCHAR(20) REFERENCES carbon.markets(market_id),
    transaction_hash VARCHAR(66),
    buyer_address VARCHAR(42),
    seller_address VARCHAR(42),
    amount DECIMAL(15,4) NOT NULL,
    price DECIMAL(10,2) NOT NULL,
    total_cost DECIMAL(15,2) NOT NULL,
    status VARCHAR(20) DEFAULT 'pending',
    executed_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Blockchain accounts table
CREATE TABLE IF NOT EXISTS blockchain.accounts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    address VARCHAR(42) UNIQUE NOT NULL,
    balance DECIMAL(30,0) DEFAULT 0, -- Wei units
    nonce INTEGER DEFAULT 0,
    account_type VARCHAR(20) DEFAULT 'user', -- user, contract, system
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Blockchain transactions table
CREATE TABLE IF NOT EXISTS blockchain.transactions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    hash VARCHAR(66) UNIQUE NOT NULL,
    from_address VARCHAR(42) NOT NULL,
    to_address VARCHAR(42),
    value DECIMAL(30,0) DEFAULT 0,
    gas DECIMAL(10,0) NOT NULL,
    gas_price DECIMAL(20,0) NOT NULL,
    gas_used DECIMAL(10,0),
    nonce INTEGER NOT NULL,
    block_number INTEGER,
    block_hash VARCHAR(66),
    transaction_index INTEGER,
    status VARCHAR(10) DEFAULT 'pending', -- pending, success, failed
    input_data TEXT,
    logs JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Smart contracts table
CREATE TABLE IF NOT EXISTS blockchain.contracts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    address VARCHAR(42) UNIQUE NOT NULL,
    name VARCHAR(100) NOT NULL,
    symbol VARCHAR(20),
    contract_type VARCHAR(50) NOT NULL, -- ERC20, ERC721, custom
    bytecode TEXT,
    abi JSONB,
    creator_address VARCHAR(42) NOT NULL,
    deployment_tx_hash VARCHAR(66),
    is_verified BOOLEAN DEFAULT false,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Users table
CREATE TABLE IF NOT EXISTS users.profiles (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id VARCHAR(100) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE,
    wallet_address VARCHAR(42) UNIQUE,
    display_name VARCHAR(100),
    avatar_url TEXT,
    country_code VARCHAR(2),
    timezone VARCHAR(50),
    preferences JSONB DEFAULT '{}',
    is_active BOOLEAN DEFAULT true,
    verified_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- User carbon footprint tracking
CREATE TABLE IF NOT EXISTS users.carbon_footprint (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id VARCHAR(100) REFERENCES users.profiles(user_id),
    activity_type VARCHAR(50) NOT NULL, -- transport, energy, waste
    activity_amount DECIMAL(12,4) NOT NULL,
    activity_unit VARCHAR(20) NOT NULL,
    carbon_amount DECIMAL(10,6) NOT NULL, -- tCO2e
    recorded_date DATE NOT NULL,
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Insert test weather stations
INSERT INTO weather.stations (station_id, name, latitude, longitude, elevation, country_code, region, is_active) VALUES
('WS001', 'Central Park Weather Station', 40.7829, -73.9654, 42, 'US', 'New York', true),
('WS002', 'Golden Gate Weather Station', 37.8199, -122.4783, 67, 'US', 'California', true),
('WS003', 'Miami Beach Weather Station', 25.7907, -80.1300, 2, 'US', 'Florida', true),
('WS004', 'Denver Mountain Station', 39.7392, -104.9903, 1609, 'US', 'Colorado', true),
('WS005', 'Seattle Coastal Station', 47.6062, -122.3321, 56, 'US', 'Washington', true),
('WS006', 'Phoenix Desert Station', 33.4484, -112.0740, 331, 'US', 'Arizona', true),
('WS007', 'Chicago Lakefront Station', 41.8781, -87.6298, 179, 'US', 'Illinois', true),
('WS008', 'Portland Forest Station', 45.5152, -122.6784, 15, 'US', 'Oregon', true),
('WS009', 'Boston Harbor Station', 42.3601, -71.0589, 43, 'US', 'Massachusetts', true),
('WS010', 'Houston Energy Station', 29.7604, -95.3698, 13, 'US', 'Texas', true)
ON CONFLICT (station_id) DO NOTHING;

-- Insert sample weather readings (last 7 days)
WITH date_series AS (
    SELECT generate_series(
        CURRENT_DATE - INTERVAL '7 days',
        CURRENT_DATE,
        INTERVAL '6 hours'
    )::timestamp AS reading_time
),
station_readings AS (
    SELECT 
        ws.station_id,
        ds.reading_time,
        -- Generate realistic temperature based on location
        CASE 
            WHEN ws.station_id = 'WS003' THEN 24 + (RANDOM() * 8) -- Miami (warm)
            WHEN ws.station_id = 'WS006' THEN 30 + (RANDOM() * 12) -- Phoenix (hot)
            WHEN ws.station_id = 'WS004' THEN 5 + (RANDOM() * 15) -- Denver (variable)
            WHEN ws.station_id = 'WS008' THEN 10 + (RANDOM() * 10) -- Portland (mild)
            ELSE 15 + (RANDOM() * 10) -- Default range
        END AS temp,
        50 + (RANDOM() * 40) AS humidity,
        1000 + (RANDOM() * 50) AS pressure,
        RANDOM() * 15 AS wind_speed,
        (RANDOM() * 360)::INTEGER AS wind_dir,
        CASE WHEN RANDOM() < 0.3 THEN RANDOM() * 5 ELSE 0 END AS precip,
        5 + (RANDOM() * 15) AS visibility,
        CASE 
            WHEN RANDOM() < 0.4 THEN 'clear'
            WHEN RANDOM() < 0.6 THEN 'cloudy'
            WHEN RANDOM() < 0.8 THEN 'overcast'
            ELSE 'rainy'
        END AS conditions
    FROM weather.stations ws
    CROSS JOIN date_series ds
    WHERE ws.is_active = true
)
INSERT INTO weather.readings (
    station_id, recorded_at, temperature, humidity, pressure,
    wind_speed, wind_direction, precipitation, visibility, conditions
)
SELECT 
    station_id, reading_time, temp, humidity, pressure,
    wind_speed, wind_dir, precip, visibility, conditions
FROM station_readings
ON CONFLICT DO NOTHING;

-- Insert carbon markets
INSERT INTO carbon.markets (market_id, name, type, base_price, current_price, currency, unit) VALUES
('VCS', 'Verified Carbon Standard', 'voluntary', 15.50, 15.75, 'USD', 'tCO2e'),
('GS', 'Gold Standard', 'voluntary', 22.30, 21.95, 'USD', 'tCO2e'),
('CAR', 'Climate Action Reserve', 'voluntary', 18.75, 19.20, 'USD', 'tCO2e'),
('EU-ETS', 'EU Emissions Trading System', 'compliance', 85.20, 87.45, 'EUR', 'tCO2e'),
('WCI', 'Western Climate Initiative', 'compliance', 28.45, 27.80, 'USD', 'tCO2e'),
('NATURE', 'Nature-Based Solutions', 'premium', 45.80, 46.30, 'USD', 'tCO2e')
ON CONFLICT (market_id) DO NOTHING;

-- Insert sample carbon transactions (last 30 days)
WITH transaction_data AS (
    SELECT 
        cm.market_id,
        encode(gen_random_bytes(32), 'hex') AS tx_hash,
        '0x' || encode(gen_random_bytes(20), 'hex') AS buyer,
        '0x' || encode(gen_random_bytes(20), 'hex') AS seller,
        (1 + RANDOM() * 100)::DECIMAL(15,4) AS amount,
        cm.current_price * (0.95 + RANDOM() * 0.1) AS price,
        CURRENT_TIMESTAMP - (RANDOM() * INTERVAL '30 days') AS created_time,
        CASE 
            WHEN RANDOM() < 0.8 THEN 'completed'
            WHEN RANDOM() < 0.95 THEN 'pending'
            ELSE 'failed'
        END AS tx_status
    FROM carbon.markets cm
    CROSS JOIN generate_series(1, 20) -- 20 transactions per market
    WHERE cm.is_active = true
)
INSERT INTO carbon.transactions (
    market_id, transaction_hash, buyer_address, seller_address,
    amount, price, total_cost, status, executed_at, created_at
)
SELECT 
    market_id, '0x' || tx_hash, buyer, seller,
    amount, price, amount * price, tx_status,
    CASE WHEN tx_status = 'completed' THEN created_time ELSE NULL END,
    created_time
FROM transaction_data;

-- Insert blockchain test accounts
WITH account_data AS (
    SELECT 
        '0x' || encode(gen_random_bytes(20), 'hex') AS address,
        (RANDOM() * 1000 * 10^18)::DECIMAL(30,0) AS balance, -- Random ETH balance in wei
        (RANDOM() * 100)::INTEGER AS nonce,
        CASE 
            WHEN RANDOM() < 0.8 THEN 'user'
            WHEN RANDOM() < 0.95 THEN 'contract'
            ELSE 'system'
        END AS acc_type
    FROM generate_series(1, 50)
)
INSERT INTO blockchain.accounts (address, balance, nonce, account_type)
SELECT address, balance, nonce, acc_type FROM account_data;

-- Insert test smart contracts
INSERT INTO blockchain.contracts (
    address, name, symbol, contract_type, creator_address, is_verified
) VALUES
('0x' || encode(gen_random_bytes(20), 'hex'), 'WRFCoin Token', 'WRF', 'ERC20', 
 (SELECT address FROM blockchain.accounts LIMIT 1), true),
('0x' || encode(gen_random_bytes(20), 'hex'), 'Carbon Credit NFT', 'CCN', 'ERC721',
 (SELECT address FROM blockchain.accounts LIMIT 1), true),
('0x' || encode(gen_random_bytes(20), 'hex'), 'Weather Oracle', 'WO', 'custom',
 (SELECT address FROM blockchain.accounts LIMIT 1), true),
('0x' || encode(gen_random_bytes(20), 'hex'), 'AMM Pool', 'AMP', 'custom',
 (SELECT address FROM blockchain.accounts LIMIT 1), false);

-- Insert test users
WITH user_data AS (
    SELECT 
        'user_' || i AS user_id,
        'user' || i || '@example.com' AS email,
        ba.address AS wallet_addr,
        'Test User ' || i AS display_name,
        CASE (i % 5)
            WHEN 0 THEN 'US'
            WHEN 1 THEN 'CA'
            WHEN 2 THEN 'GB'
            WHEN 3 THEN 'DE'
            ELSE 'AU'
        END AS country,
        CURRENT_TIMESTAMP - (RANDOM() * INTERVAL '365 days') AS join_time
    FROM generate_series(1, 25) i
    CROSS JOIN LATERAL (
        SELECT address FROM blockchain.accounts 
        WHERE account_type = 'user' 
        ORDER BY RANDOM() 
        LIMIT 1
    ) ba
)
INSERT INTO users.profiles (
    user_id, email, wallet_address, display_name, 
    country_code, verified_at, created_at
)
SELECT 
    user_id, email, wallet_addr, display_name,
    country, join_time, join_time
FROM user_data;

-- Insert carbon footprint tracking data
WITH footprint_data AS (
    SELECT 
        up.user_id,
        CASE (RANDOM() * 3)::INTEGER
            WHEN 0 THEN 'transport'
            WHEN 1 THEN 'energy'
            ELSE 'waste'
        END AS activity,
        RANDOM() * 1000 AS amount,
        CASE (RANDOM() * 3)::INTEGER
            WHEN 0 THEN 'km'
            WHEN 1 THEN 'kwh'
            ELSE 'kg'
        END AS unit,
        CURRENT_DATE - (RANDOM() * 90)::INTEGER AS record_date
    FROM users.profiles up
    CROSS JOIN generate_series(1, 10) -- 10 records per user
    WHERE up.is_active = true
)
INSERT INTO users.carbon_footprint (
    user_id, activity_type, activity_amount, activity_unit,
    carbon_amount, recorded_date
)
SELECT 
    user_id, activity, amount, unit,
    -- Simple carbon calculation
    CASE activity
        WHEN 'transport' THEN amount * 0.0002 -- kg CO2 per km
        WHEN 'energy' THEN amount * 0.0005   -- kg CO2 per kWh  
        ELSE amount * 0.001                  -- kg CO2 per kg waste
    END,
    record_date
FROM footprint_data;

-- Create indexes for better performance
CREATE INDEX IF NOT EXISTS idx_weather_readings_station_time ON weather.readings(station_id, recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_carbon_transactions_market_time ON carbon.transactions(market_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_blockchain_transactions_address ON blockchain.transactions(from_address, to_address);
CREATE INDEX IF NOT EXISTS idx_blockchain_transactions_block ON blockchain.transactions(block_number DESC);
CREATE INDEX IF NOT EXISTS idx_users_carbon_footprint_user_date ON users.carbon_footprint(user_id, recorded_date DESC);

-- Update table statistics
ANALYZE;

-- Create views for common queries
CREATE OR REPLACE VIEW weather.latest_readings AS
SELECT DISTINCT ON (station_id) 
    station_id, recorded_at, temperature, humidity, pressure,
    wind_speed, wind_direction, precipitation, conditions
FROM weather.readings
ORDER BY station_id, recorded_at DESC;

CREATE OR REPLACE VIEW carbon.market_summary AS
SELECT 
    m.market_id,
    m.name,
    m.type,
    m.current_price,
    m.currency,
    COUNT(t.id) AS total_transactions,
    COALESCE(SUM(t.amount), 0) AS total_volume,
    COALESCE(AVG(t.price), m.current_price) AS avg_price
FROM carbon.markets m
LEFT JOIN carbon.transactions t ON m.market_id = t.market_id
WHERE m.is_active = true
GROUP BY m.market_id, m.name, m.type, m.current_price, m.currency;

CREATE OR REPLACE VIEW users.carbon_summary AS
SELECT 
    p.user_id,
    p.display_name,
    COUNT(cf.id) AS total_activities,
    SUM(cf.carbon_amount) AS total_carbon_footprint,
    AVG(cf.carbon_amount) AS avg_daily_carbon
FROM users.profiles p
LEFT JOIN users.carbon_footprint cf ON p.user_id = cf.user_id
WHERE p.is_active = true
GROUP BY p.user_id, p.display_name;

-- Log completion
DO $$
BEGIN
    RAISE NOTICE 'PostgreSQL test data initialization completed successfully';
    RAISE NOTICE 'Created % weather stations', (SELECT COUNT(*) FROM weather.stations);
    RAISE NOTICE 'Created % weather readings', (SELECT COUNT(*) FROM weather.readings);
    RAISE NOTICE 'Created % carbon markets', (SELECT COUNT(*) FROM carbon.markets);
    RAISE NOTICE 'Created % carbon transactions', (SELECT COUNT(*) FROM carbon.transactions);
    RAISE NOTICE 'Created % blockchain accounts', (SELECT COUNT(*) FROM blockchain.accounts);
    RAISE NOTICE 'Created % smart contracts', (SELECT COUNT(*) FROM blockchain.contracts);
    RAISE NOTICE 'Created % user profiles', (SELECT COUNT(*) FROM users.profiles);
    RAISE NOTICE 'Created % carbon footprint records', (SELECT COUNT(*) FROM users.carbon_footprint);
END $$;