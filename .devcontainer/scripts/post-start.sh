#!/bin/bash

# WRFCoin Post-Start Script
# This script runs every time the devcontainer starts (including restarts)

set -e

echo "🔄 WRFCoin development container post-start setup..."

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_status() {
    echo -e "${BLUE}[POST-START]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

cd /workspace

print_status "Checking service dependencies..."

# Function to wait for service
wait_for_service() {
    local service_name=$1
    local host=$2
    local port=$3
    local max_attempts=${4:-30}
    local attempt=0
    
    print_status "Waiting for $service_name to be ready..."
    while ! nc -z "$host" "$port" 2>/dev/null && [ $attempt -lt $max_attempts ]; do
        sleep 1
        ((attempt++))
        if [ $((attempt % 10)) -eq 0 ]; then
            print_status "Still waiting for $service_name... (${attempt}/${max_attempts})"
        fi
    done
    
    if [ $attempt -lt $max_attempts ]; then
        print_success "$service_name is ready"
        return 0
    else
        print_warning "$service_name not ready after ${max_attempts} seconds"
        return 1
    fi
}

# Wait for essential services
wait_for_service "PostgreSQL" "test-db" "5432" 60
wait_for_service "Redis" "test-redis" "6379" 30

# Check if IPFS is available
if wait_for_service "IPFS" "ipfs" "5001" 15; then
    print_success "IPFS node is available"
else
    print_warning "IPFS node not ready (this is optional for most development tasks)"
fi

print_status "Refreshing environment configuration..."

# Update environment variables for current session
if [ -f .env.development ]; then
    # Export development environment variables (without overriding existing ones)
    set -a  # Export all variables
    source .env.development
    set +a  # Stop exporting
    print_success "Development environment variables loaded"
fi

print_status "Updating development dependencies..."

# Quick npm check (only if package.json is newer than node_modules)
if [ -f package.json ]; then
    if [ ! -d node_modules ] || [ package.json -nt node_modules ]; then
        print_status "Package.json updated, installing new dependencies..."
        npm install --prefer-offline --no-audit --progress=false --silent
        print_success "Dependencies updated"
    fi
fi

print_status "Checking development tools..."

# Ensure Rust components are available
if command -v rustc &> /dev/null; then
    # Make sure clippy and rustfmt are available
    rustup component add clippy rustfmt 2>/dev/null || true
    print_success "Rust toolchain ready"
fi

# Check Python documentation tools
if ! python3 -c "import mkdocs" &> /dev/null; then
    print_status "Installing Python documentation tools..."
    python3 -m pip install --user --upgrade --quiet \
        mkdocs mkdocs-material mkdocs-mermaid2-plugin \
        mkdocs-git-revision-date-localized-plugin \
        mkdocs-minify-plugin pymdown-extensions 2>/dev/null || true
fi

print_status "Starting background services..."

# Start quality monitoring if not already running
if [ -f scripts/start-quality-monitoring.sh ] && ! pgrep -f "quality.*monitor" > /dev/null; then
    print_status "Starting quality monitoring..."
    bash scripts/start-quality-monitoring.sh > /dev/null 2>&1 &
    print_success "Quality monitoring started"
fi

# Start file watchers for documentation
if [ -f scripts/build-mkdocs.sh ] && command -v inotifywait &> /dev/null; then
    print_status "Setting up documentation auto-rebuild..."
    # This will run in the background and rebuild docs when files change
    {
        inotifywait -m -r -e modify --include='\.(md|yml|yaml)$' docs/ 2>/dev/null | while read path action file; do
            if [[ "$file" =~ \.(md|yml|yaml)$ ]]; then
                echo "Documentation file changed: $file"
                bash scripts/build-mkdocs.sh > /dev/null 2>&1 || true
            fi
        done
    } &
    print_success "Documentation auto-rebuild enabled"
fi

print_status "Performing health checks..."

# Check if main services are accessible
health_check_passed=true

# Check Node.js health
if ! node -e "console.log('Node.js OK')" &> /dev/null; then
    print_error "Node.js health check failed"
    health_check_passed=false
fi

# Check database connectivity
if command -v psql &> /dev/null && nc -z test-db 5432 2>/dev/null; then
    if PGPASSWORD=password psql -h test-db -U postgres -d wrfcoin_test -c "SELECT 1;" &> /dev/null; then
        print_success "Database connectivity verified"
    else
        print_warning "Database connection issue (database might not be initialized yet)"
    fi
fi

# Check Redis connectivity
if command -v redis-cli &> /dev/null && nc -z test-redis 6379 2>/dev/null; then
    if redis-cli -h test-redis ping | grep -q "PONG"; then
        print_success "Redis connectivity verified"
    else
        print_warning "Redis connection issue"
    fi
fi

print_status "Setting up development shortcuts..."

# Create convenient development scripts
cat > /tmp/dev-shortcuts.sh << 'EOF'
#!/bin/bash
# WRFCoin Development Shortcuts

# Quick commands for common development tasks
alias wrf-start='npm run start:all'
alias wrf-test='npm run test:all'
alias wrf-build='npm run build'
alias wrf-docs='npm run docs:serve'
alias wrf-quality='npm run quality:check'
alias wrf-clean='npm run clean && docker system prune -f'

# Service-specific commands
alias wrf-api='npm run start:backend'
alias wrf-frontend='npm run start:frontend'
alias wrf-graphql='npm run start:graphql'

# Testing shortcuts
alias wrf-test-unit='npm run test:unit'
alias wrf-test-e2e='npm run test:e2e'
alias wrf-test-contracts='npm run test:smart-contracts'

# Development utilities
alias wrf-logs='docker-compose logs -f'
alias wrf-restart='docker-compose restart'
alias wrf-reset='docker-compose down && docker-compose up -d'

echo "WRFCoin development shortcuts loaded!"
echo "Available commands:"
echo "  wrf-start    - Start all services"
echo "  wrf-test     - Run all tests"
echo "  wrf-docs     - Start documentation server"
echo "  wrf-quality  - Check code quality"
echo "  wrf-logs     - View service logs"
echo "  wrf-restart  - Restart services"
EOF

# Source shortcuts if they exist
if [ -f /tmp/dev-shortcuts.sh ]; then
    source /tmp/dev-shortcuts.sh
    
    # Add to bashrc if not already there
    if ! grep -q "WRFCoin development shortcuts" ~/.bashrc; then
        echo "" >> ~/.bashrc
        echo "# WRFCoin development shortcuts" >> ~/.bashrc
        cat /tmp/dev-shortcuts.sh >> ~/.bashrc
    fi
fi

print_status "Displaying system information..."
echo ""
echo -e "${BLUE}🖥️  System Information:${NC}"
echo "  Node.js: $(node --version 2>/dev/null || echo 'not available')"
echo "  npm: $(npm --version 2>/dev/null || echo 'not available')"
echo "  Rust: $(rustc --version 2>/dev/null || echo 'not available')"
echo "  Python: $(python3 --version 2>/dev/null || echo 'not available')"
echo "  Docker: $(docker --version 2>/dev/null | head -1 || echo 'not available')"
echo ""

echo -e "${BLUE}🌐 Service Status:${NC}"
echo "  Database: $(nc -z test-db 5432 2>/dev/null && echo '✅ Ready' || echo '❌ Not ready')"
echo "  Redis: $(nc -z test-redis 6379 2>/dev/null && echo '✅ Ready' || echo '❌ Not ready')"
echo "  IPFS: $(nc -z ipfs 5001 2>/dev/null && echo '✅ Ready' || echo '⚠️  Optional')"
echo ""

echo -e "${BLUE}🔗 Available Services:${NC}"
echo "  Frontend: http://localhost:3000"
echo "  API Server: http://localhost:4001"
echo "  GraphQL: http://localhost:4001/graphql"
echo "  Documentation: http://localhost:8001 (when running)"
echo ""

if $health_check_passed; then
    print_success "Post-start setup completed successfully!"
else
    print_warning "Post-start setup completed with some issues"
fi

echo -e "${GREEN}Development environment is ready for coding! 🎯${NC}"
echo ""
echo -e "${YELLOW}💡 Tips:${NC}"
echo "  • Use 'wrf-start' to quickly start all services"
echo "  • Use 'wrf-test' to run all tests"
echo "  • Use 'wrf-docs' to start the documentation server"
echo "  • Check 'wrf-logs' if services aren't working correctly"