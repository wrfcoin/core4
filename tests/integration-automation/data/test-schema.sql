-- Integration Test Database Schema
-- Agent 6 - Integration Test Automation Specialist
-- 
-- Minimal schema for integration testing with Agent 4's mock services
-- Compatible with Agent 1's Pact contract requirements

-- Users table for authentication and user management tests
CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    username VARCHAR(255) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    wallet_address VARCHAR(255) UNIQUE NOT NULL,
    status VARCHAR(50) DEFAULT 'active',
    role VARCHAR(50) DEFAULT 'user',
    is_verified BOOLEAN DEFAULT false,
    reputation INTEGER DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    -- Constraints for wallet address format validation
    CONSTRAINT valid_wallet_address CHECK (wallet_address ~ '^0x[a-fA-F0-9]{40}$' OR wallet_address ~ '^wrf[a-fA-F0-9]{40}$'),
    CONSTRAINT valid_status CHECK (status IN ('active', 'inactive', 'suspended', 'pending')),
    CONSTRAINT valid_role CHECK (role IN ('user', 'admin', 'moderator', 'station_operator'))
);

-- Weather data table for weather oracle integration tests
CREATE TABLE IF NOT EXISTS weather_data (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    station_id VARCHAR(255) NOT NULL,
    measurements JSONB NOT NULL,
    carbon_metrics JSONB,
    quality_score DECIMAL(3,2) DEFAULT 0.0,
    timestamp TIMESTAMP NOT NULL,
    processed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    -- Indexes for performance
    INDEX idx_weather_station_timestamp (station_id, timestamp),
    INDEX idx_weather_timestamp (timestamp),
    
    -- Constraints
    CONSTRAINT valid_quality_score CHECK (quality_score >= 0.0 AND quality_score <= 1.0)
);

-- Transactions table for blockchain integration tests
CREATE TABLE IF NOT EXISTS transactions (
    id SERIAL PRIMARY KEY,
    tx_id VARCHAR(255) UNIQUE NOT NULL,
    from_address VARCHAR(255) NOT NULL,
    to_address VARCHAR(255) NOT NULL,
    amount BIGINT NOT NULL,
    fee BIGINT DEFAULT 0,
    status VARCHAR(50) DEFAULT 'pending',
    type VARCHAR(50) NOT NULL,
    data JSONB,
    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    block_number BIGINT,
    confirmations INTEGER DEFAULT 0,
    
    -- Constraints
    CONSTRAINT valid_tx_id CHECK (tx_id ~ '^0x[a-fA-F0-9]{64}$'),
    CONSTRAINT valid_from_address CHECK (from_address ~ '^0x[a-fA-F0-9]{40}$' OR from_address ~ '^wrf[a-fA-F0-9]{40}$'),
    CONSTRAINT valid_to_address CHECK (to_address ~ '^0x[a-fA-F0-9]{40}$' OR to_address ~ '^wrf[a-fA-F0-9]{40}$'),
    CONSTRAINT valid_amount CHECK (amount >= 0),
    CONSTRAINT valid_fee CHECK (fee >= 0),
    CONSTRAINT valid_status CHECK (status IN ('pending', 'confirmed', 'failed', 'cancelled')),
    CONSTRAINT valid_type CHECK (type IN ('transfer', 'weather_reward', 'carbon_credit', 'staking', 'governance'))
);

-- Carbon credits table for carbon tracking tests
CREATE TABLE IF NOT EXISTS carbon_credits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    amount DECIMAL(10,6) NOT NULL,
    source VARCHAR(255) NOT NULL,
    metadata JSONB,
    status VARCHAR(50) DEFAULT 'pending',
    issued_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP,
    retired_at TIMESTAMP,
    
    -- Constraints
    CONSTRAINT valid_amount CHECK (amount > 0),
    CONSTRAINT valid_source CHECK (source IN ('weather_monitoring', 'carbon_offset', 'renewable_energy', 'reforestation')),
    CONSTRAINT valid_status CHECK (status IN ('pending', 'verified', 'issued', 'retired', 'cancelled'))
);

-- User sessions table for authentication testing
CREATE TABLE IF NOT EXISTS user_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    session_token VARCHAR(255) UNIQUE NOT NULL,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_activity TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    ip_address INET,
    user_agent TEXT,
    is_active BOOLEAN DEFAULT true
);

-- API keys table for service integration testing
CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    key_name VARCHAR(255) NOT NULL,
    api_key_hash VARCHAR(255) UNIQUE NOT NULL,
    permissions JSONB DEFAULT '[]'::jsonb,
    rate_limit INTEGER DEFAULT 1000,
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP,
    last_used_at TIMESTAMP
);

-- Weather stations table for station management tests
CREATE TABLE IF NOT EXISTS weather_stations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    station_id VARCHAR(255) UNIQUE NOT NULL,
    owner_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    location JSONB NOT NULL, -- {lat, lng, altitude, address}
    configuration JSONB DEFAULT '{}'::jsonb,
    status VARCHAR(50) DEFAULT 'active',
    last_report TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    -- Constraints
    CONSTRAINT valid_status CHECK (status IN ('active', 'inactive', 'maintenance', 'error'))
);

-- Create indexes for performance
CREATE INDEX IF NOT EXISTS idx_users_wallet_address ON users(wallet_address);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_status ON users(status);

CREATE INDEX IF NOT EXISTS idx_weather_data_station ON weather_data(station_id);
CREATE INDEX IF NOT EXISTS idx_weather_data_timestamp ON weather_data(timestamp);
CREATE INDEX IF NOT EXISTS idx_weather_data_quality ON weather_data(quality_score);

CREATE INDEX IF NOT EXISTS idx_transactions_from_address ON transactions(from_address);
CREATE INDEX IF NOT EXISTS idx_transactions_to_address ON transactions(to_address);
CREATE INDEX IF NOT EXISTS idx_transactions_status ON transactions(status);
CREATE INDEX IF NOT EXISTS idx_transactions_type ON transactions(type);
CREATE INDEX IF NOT EXISTS idx_transactions_timestamp ON transactions(timestamp);

CREATE INDEX IF NOT EXISTS idx_carbon_credits_user ON carbon_credits(user_id);
CREATE INDEX IF NOT EXISTS idx_carbon_credits_status ON carbon_credits(status);
CREATE INDEX IF NOT EXISTS idx_carbon_credits_source ON carbon_credits(source);

CREATE INDEX IF NOT EXISTS idx_user_sessions_user ON user_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_user_sessions_token ON user_sessions(session_token);
CREATE INDEX IF NOT EXISTS idx_user_sessions_active ON user_sessions(is_active);

CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(api_key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_active ON api_keys(is_active);

CREATE INDEX IF NOT EXISTS idx_weather_stations_owner ON weather_stations(owner_id);
CREATE INDEX IF NOT EXISTS idx_weather_stations_status ON weather_stations(status);

-- Create updated_at trigger function
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Apply updated_at triggers to relevant tables
CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_weather_stations_updated_at BEFORE UPDATE ON weather_stations
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Insert some test data for integration testing
INSERT INTO users (username, email, wallet_address, status, role) VALUES
    ('integration_test_user', 'test@wrfcoin.integration', '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9f1234', 'active', 'user'),
    ('station_operator_test', 'station@wrfcoin.integration', '0x856f48C9247d7b8e3D4B8F5e2C9A7b8E4C5F1234', 'active', 'station_operator'),
    ('admin_test', 'admin@wrfcoin.integration', '0x123456789ABCDEF123456789ABCDEF1234567890', 'active', 'admin')
ON CONFLICT (username) DO NOTHING;

INSERT INTO weather_stations (station_id, owner_id, name, location, status) VALUES
    ('station-integration-001', 1, 'Integration Test Station 1', '{"lat": 40.7128, "lng": -74.0060, "altitude": 10, "address": "New York, NY"}', 'active'),
    ('station-integration-002', 2, 'Integration Test Station 2', '{"lat": 34.0522, "lng": -118.2437, "altitude": 100, "address": "Los Angeles, CA"}', 'active')
ON CONFLICT (station_id) DO NOTHING;