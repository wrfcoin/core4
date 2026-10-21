-- Contract Testing Database Initialization
-- Creates the necessary tables and indexes for contract testing

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- Users table
CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT,
    wallet_address TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    last_login TIMESTAMP,
    status TEXT NOT NULL DEFAULT 'active',
    role TEXT NOT NULL DEFAULT 'user',
    preferences JSONB,
    profile_picture TEXT,
    reputation INTEGER DEFAULT 0,
    display_name TEXT,
    bio TEXT,
    avatar TEXT,
    location TEXT,
    is_verified BOOLEAN DEFAULT FALSE,
    last_active_at TIMESTAMP
);

-- Weather data table (consolidated schema)
CREATE TABLE IF NOT EXISTS weather_data (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    station_id VARCHAR(50) NOT NULL,
    measurements JSONB NOT NULL,
    carbon_metrics JSONB,
    quality_score REAL,
    timestamp TIMESTAMP NOT NULL,
    processed_at TIMESTAMP DEFAULT NOW()
);

-- Transactions table
CREATE TABLE IF NOT EXISTS transactions (
    id SERIAL PRIMARY KEY,
    tx_id TEXT NOT NULL UNIQUE,
    from_address TEXT NOT NULL,
    to_address TEXT NOT NULL,
    amount BIGINT NOT NULL,
    fee BIGINT NOT NULL,
    timestamp TIMESTAMP NOT NULL DEFAULT NOW(),
    status TEXT NOT NULL,
    type TEXT NOT NULL,
    block_id INTEGER,
    data JSONB
);

-- Carbon credits table
CREATE TABLE IF NOT EXISTS carbon_credits (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id INTEGER REFERENCES users(id),
    amount DECIMAL(18, 8) NOT NULL,
    source VARCHAR(100) NOT NULL,
    metadata JSONB,
    status VARCHAR(20) DEFAULT 'pending',
    issued_at TIMESTAMP DEFAULT NOW()
);

-- Blocks table
CREATE TABLE IF NOT EXISTS blocks (
    id SERIAL PRIMARY KEY,
    block_id TEXT NOT NULL UNIQUE,
    height INTEGER NOT NULL,
    timestamp TIMESTAMP NOT NULL DEFAULT NOW(),
    hash TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    transaction_count INTEGER NOT NULL,
    difficulty INTEGER NOT NULL,
    size INTEGER NOT NULL,
    validator TEXT,
    merkle_root TEXT NOT NULL,
    nonce TEXT,
    block_type TEXT NOT NULL DEFAULT 'standard',
    signature TEXT
);

-- MFA and security tables for testing
CREATE TABLE IF NOT EXISTS user_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id INTEGER REFERENCES users(id),
    session_token TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP DEFAULT NOW(),
    last_accessed_at TIMESTAMP DEFAULT NOW(),
    ip_address INET,
    user_agent TEXT,
    is_active BOOLEAN DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS trusted_devices (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id INTEGER REFERENCES users(id),
    device_hash TEXT NOT NULL,
    fingerprint JSONB NOT NULL,
    ip_address INET,
    first_seen TIMESTAMP DEFAULT NOW(),
    last_seen TIMESTAMP DEFAULT NOW(),
    trust_level TEXT DEFAULT 'pending',
    device_name TEXT
);

CREATE TABLE IF NOT EXISTS security_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id INTEGER REFERENCES users(id),
    event_type TEXT NOT NULL,
    severity TEXT NOT NULL,
    details JSONB,
    ip_address INET,
    user_agent TEXT,
    timestamp TIMESTAMP DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_wallet_address ON users(wallet_address);

CREATE INDEX IF NOT EXISTS idx_weather_data_station_timestamp ON weather_data(station_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_weather_data_measurements_gin ON weather_data USING GIN(measurements);
CREATE INDEX IF NOT EXISTS idx_weather_data_carbon_metrics_gin ON weather_data USING GIN(carbon_metrics);

CREATE INDEX IF NOT EXISTS idx_transactions_tx_id ON transactions(tx_id);
CREATE INDEX IF NOT EXISTS idx_transactions_from_address ON transactions(from_address);
CREATE INDEX IF NOT EXISTS idx_transactions_to_address ON transactions(to_address);
CREATE INDEX IF NOT EXISTS idx_transactions_timestamp ON transactions(timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_carbon_credits_user_id ON carbon_credits(user_id);
CREATE INDEX IF NOT EXISTS idx_carbon_credits_source ON carbon_credits(source);
CREATE INDEX IF NOT EXISTS idx_carbon_credits_status ON carbon_credits(status);

CREATE INDEX IF NOT EXISTS idx_blocks_height ON blocks(height);
CREATE INDEX IF NOT EXISTS idx_blocks_timestamp ON blocks(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_blocks_hash ON blocks(hash);

CREATE INDEX IF NOT EXISTS idx_user_sessions_user_id ON user_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_user_sessions_token ON user_sessions(session_token);
CREATE INDEX IF NOT EXISTS idx_user_sessions_active ON user_sessions(is_active) WHERE is_active = TRUE;

CREATE INDEX IF NOT EXISTS idx_trusted_devices_user_id ON trusted_devices(user_id);
CREATE INDEX IF NOT EXISTS idx_trusted_devices_hash ON trusted_devices(device_hash);

CREATE INDEX IF NOT EXISTS idx_security_events_user_id ON security_events(user_id);
CREATE INDEX IF NOT EXISTS idx_security_events_type ON security_events(event_type);
CREATE INDEX IF NOT EXISTS idx_security_events_timestamp ON security_events(timestamp DESC);

-- Sample test data for contract testing
INSERT INTO users (username, email, wallet_address, status, role) VALUES
('testuser1', 'test1@wrfcoin.com', '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234', 'active', 'user'),
('testuser2', 'test2@wrfcoin.com', '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234', 'active', 'user'),
('admin', 'admin@wrfcoin.com', '0x123f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F5678', 'active', 'admin')
ON CONFLICT (username) DO NOTHING;

-- Sample weather data
INSERT INTO weather_data (station_id, measurements, carbon_metrics, quality_score, timestamp) VALUES
('station-sample', 
 '{"temperature": 22.5, "humidity": 65.0, "pressure": 1013.25, "windSpeed": 3.2, "windDirection": 180}',
 '{"co2Equivalent": 0.05, "carbonCreditsEarned": 0.005, "calculationMethod": "weather_monitoring_ipcc_2021"}',
 0.95,
 NOW() - INTERVAL '1 hour'),
('station-sample',
 '{"temperature": 23.1, "humidity": 62.0, "pressure": 1012.8, "windSpeed": 2.8, "windDirection": 185}',
 '{"co2Equivalent": 0.048, "carbonCreditsEarned": 0.0048, "calculationMethod": "weather_monitoring_ipcc_2021"}',
 0.92,
 NOW() - INTERVAL '30 minutes')
ON CONFLICT DO NOTHING;

-- Sample transactions
INSERT INTO transactions (tx_id, from_address, to_address, amount, fee, status, type) VALUES
('0xsample1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
 '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234',
 '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234',
 1000000000000000000,
 21000,
 'confirmed',
 'transfer')
ON CONFLICT (tx_id) DO NOTHING;

-- Sample carbon credits
INSERT INTO carbon_credits (user_id, amount, source, metadata, status) VALUES
(1, 1.5678, 'weather_monitoring', 
 '{"calculationMethod": "weather_monitoring_ipcc_2021", "stationId": "station-sample", "co2Offset": 1.5678, "verificationStandard": "vm0012"}',
 'verified'),
(2, 0.8945, 'weather_monitoring',
 '{"calculationMethod": "weather_monitoring_ipcc_2021", "stationId": "station-sample", "co2Offset": 0.8945, "verificationStandard": "vm0012"}',
 'pending')
ON CONFLICT DO NOTHING;

-- Create updated_at trigger function
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Add updated_at triggers
DO $$ 
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_users_updated_at') THEN
        CREATE TRIGGER update_users_updated_at 
            BEFORE UPDATE ON users 
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;