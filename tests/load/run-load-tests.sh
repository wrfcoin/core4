#!/bin/bash

# WRFCoin K6 Load Testing Execution Script
# Comprehensive load testing for all WRFCoin microservices
# 
# Usage:
#   ./run-load-tests.sh [scenario] [environment]
#   ./run-load-tests.sh sensor-flood development
#   ./run-load-tests.sh all production

set -e

# Script configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
REPORTS_DIR="$SCRIPT_DIR/reports"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default configuration
DEFAULT_SCENARIO="standard"
DEFAULT_ENVIRONMENT="development"
DEFAULT_DURATION="15m"
DEFAULT_MAX_VUS="200"

# Available test scenarios
declare -A SCENARIOS
SCENARIOS[smoke]="scenarios/sensor-flood-test.js --env K6_SCENARIO=smoke --duration 30s --vus 1"
SCENARIOS[standard]="scenarios/sensor-flood-test.js --env K6_SCENARIO=standard --duration 15m"
SCENARIOS[sensor-flood]="scenarios/sensor-flood-test.js --env K6_SCENARIO=sensorFlood --duration 15m"
SCENARIOS[cross-service]="scenarios/cross-service-integration.js --duration 20m"
SCENARIOS[database-stress]="scenarios/database-stress.js --duration 15m"
SCENARIOS[progressive-load]="scenarios/progressive-load.js --duration 35m"

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
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

# Function to check prerequisites
check_prerequisites() {
    print_status "Checking prerequisites..."
    
    # Check if K6 is installed
    if ! command -v k6 &> /dev/null; then
        print_error "K6 is not installed. Please install K6 first."
        echo "Visit: https://k6.io/docs/getting-started/installation/"
        exit 1
    fi
    
    # Check K6 version
    K6_VERSION=$(k6 version | head -n 1)
    print_status "Found $K6_VERSION"
    
    # Check if Docker is running (for services)
    if ! docker info &> /dev/null; then
        print_warning "Docker is not running. Some services may not be available."
    fi
    
    # Create reports directory
    mkdir -p "$REPORTS_DIR"
    print_status "Reports will be saved to: $REPORTS_DIR"
}

# Function to check service health
check_service_health() {
    print_status "Checking WRFCoin service health..."
    
    local services=("4001:Main API" "3002:IoT Sensors" "4000:Wallet" "3000:Cross-Chain" "8000:Governance")
    local healthy_services=0
    
    for service in "${services[@]}"; do
        IFS=':' read -r port name <<< "$service"
        
        if curl -s "http://localhost:$port/health" &> /dev/null; then
            print_success "$name service (port $port) is healthy"
            ((healthy_services++))
        else
            print_warning "$name service (port $port) is not responding"
        fi
    done
    
    if [ $healthy_services -lt 3 ]; then
        print_error "Less than 3 services are healthy. Consider starting more services before load testing."
        read -p "Continue anyway? (y/N): " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    else
        print_success "$healthy_services services are healthy and ready for load testing"
    fi
}

# Function to setup environment variables
setup_environment() {
    local environment=$1
    print_status "Setting up environment: $environment"
    
    # Load environment-specific configuration
    if [ -f "$SCRIPT_DIR/.env.k6.$environment" ]; then
        print_status "Loading environment file: .env.k6.$environment"
        export $(cat "$SCRIPT_DIR/.env.k6.$environment" | grep -v '^#' | xargs)
    elif [ -f "$SCRIPT_DIR/.env.k6" ]; then
        print_status "Loading default environment file: .env.k6"
        export $(cat "$SCRIPT_DIR/.env.k6" | grep -v '^#' | xargs)
    else
        print_warning "No environment file found, using defaults"
    fi
    
    # Set essential environment variables
    export ENVIRONMENT="${environment}"
    export K6_OUT="${K6_OUT:-json=${REPORTS_DIR}/k6_results_${TIMESTAMP}.json}"
    export K6_PROMETHEUS_ENABLED="${K6_PROMETHEUS_ENABLED:-true}"
    export K6_INFLUXDB_ENABLED="${K6_INFLUXDB_ENABLED:-false}"
}

# Function to start monitoring
start_monitoring() {
    print_status "Starting monitoring stack..."
    
    # Start Prometheus and Grafana if available
    if [ -f "$PROJECT_ROOT/infra/monitoring/docker-compose.monitoring.yml" ]; then
        cd "$PROJECT_ROOT"
        docker-compose -f infra/monitoring/docker-compose.monitoring.yml up -d prometheus grafana || true
        print_status "Monitoring stack started. Grafana available at: http://localhost:3004"
        cd "$SCRIPT_DIR"
    fi
}

# Function to run specific load test scenario
run_scenario() {
    local scenario=$1
    local scenario_config="${SCENARIOS[$scenario]}"
    
    if [ -z "$scenario_config" ]; then
        print_error "Unknown scenario: $scenario"
        print_status "Available scenarios: ${!SCENARIOS[*]}"
        exit 1
    fi
    
    print_status "Running scenario: $scenario"
    print_status "Configuration: $scenario_config"
    
    # Generate unique report filename
    local report_file="$REPORTS_DIR/${scenario}_${TIMESTAMP}"
    
    # Set output formats
    export K6_OUT="json=${report_file}.json"
    
    # Run the K6 test
    print_status "Executing K6 load test..."
    echo "================================"
    
    # Change to load test directory
    cd "$SCRIPT_DIR"
    
    # Execute K6 with scenario configuration
    if k6 run $scenario_config \
        --out "json=${report_file}.json" \
        --summary-export="${report_file}_summary.json" \
        --tag "scenario=$scenario" \
        --tag "environment=$ENVIRONMENT" \
        --tag "timestamp=$TIMESTAMP"; then
        
        print_success "Load test completed successfully"
        
        # Generate HTML report if possible
        if command -v k6-reporter &> /dev/null; then
            k6-reporter "${report_file}.json" --output "${report_file}.html"
            print_success "HTML report generated: ${report_file}.html"
        fi
        
    else
        print_error "Load test failed"
        return 1
    fi
    
    echo "================================"
    print_status "Results saved to: ${report_file}.*"
}

# Function to run all scenarios
run_all_scenarios() {
    print_status "Running all load test scenarios..."
    
    local failed_scenarios=()
    
    for scenario in "${!SCENARIOS[@]}"; do
        print_status "Starting scenario: $scenario"
        
        if run_scenario "$scenario"; then
            print_success "Scenario '$scenario' completed"
        else
            print_error "Scenario '$scenario' failed"
            failed_scenarios+=("$scenario")
        fi
        
        # Wait between scenarios to let system recover
        if [ ${#SCENARIOS[@]} -gt 1 ]; then
            print_status "Waiting 30 seconds for system recovery..."
            sleep 30
        fi
    done
    
    # Report results
    if [ ${#failed_scenarios[@]} -eq 0 ]; then
        print_success "All scenarios completed successfully!"
    else
        print_error "Failed scenarios: ${failed_scenarios[*]}"
        return 1
    fi
}

# Function to generate consolidated report
generate_report() {
    print_status "Generating consolidated test report..."
    
    local report_file="$REPORTS_DIR/consolidated_report_${TIMESTAMP}.md"
    
    cat > "$report_file" << EOF
# WRFCoin Load Testing Report
**Generated**: $(date)
**Environment**: $ENVIRONMENT
**Duration**: $DEFAULT_DURATION

## Test Summary

### System Under Test
- **Platform**: WRFCoin Weather Forecasting Blockchain
- **Services Tested**: 15+ microservices
- **Primary Target**: IoT Sensor Networks (2,000 posts/minute)

### Test Scenarios Executed
EOF
    
    for scenario in "${!SCENARIOS[@]}"; do
        echo "- **$scenario**: ${SCENARIOS[$scenario]}" >> "$report_file"
    done
    
    cat >> "$report_file" << EOF

### Key Performance Metrics
- **Target Sensor Post Rate**: 2,000 posts per minute
- **Response Time Threshold**: 95% < 800ms
- **Error Rate Threshold**: < 1%
- **System Resource Limits**: CPU < 80%, Memory < 85%

### Files Generated
EOF
    
    # List all report files
    find "$REPORTS_DIR" -name "*${TIMESTAMP}*" -type f | while read file; do
        echo "- $(basename "$file")" >> "$report_file"
    done
    
    print_success "Consolidated report generated: $report_file"
}

# Function to cleanup
cleanup() {
    print_status "Cleaning up..."
    
    # Stop monitoring containers if they were started
    if [ -f "$PROJECT_ROOT/infra/monitoring/docker-compose.monitoring.yml" ]; then
        cd "$PROJECT_ROOT"
        docker-compose -f infra/monitoring/docker-compose.monitoring.yml stop || true
        cd "$SCRIPT_DIR"
    fi
}

# Function to show usage
show_usage() {
    echo "WRFCoin K6 Load Testing Script"
    echo ""
    echo "Usage: $0 [scenario] [environment]"
    echo ""
    echo "Available scenarios:"
    for scenario in "${!SCENARIOS[@]}"; do
        echo "  $scenario"
    done
    echo "  all                 - Run all scenarios"
    echo ""
    echo "Available environments:"
    echo "  development         - Local development (default)"
    echo "  staging            - Staging environment"
    echo "  production         - Production environment"
    echo ""
    echo "Examples:"
    echo "  $0 sensor-flood development"
    echo "  $0 all production"
    echo "  $0 smoke"
    echo ""
    echo "Environment Variables:"
    echo "  K6_SCENARIO         - Override scenario"
    echo "  MAX_VUS            - Maximum virtual users"
    echo "  TEST_DURATION      - Test duration"
    echo "  MONITORING_ENABLED - Enable monitoring (true/false)"
}

# Main execution
main() {
    local scenario="${1:-$DEFAULT_SCENARIO}"
    local environment="${2:-$DEFAULT_ENVIRONMENT}"
    
    # Handle help request
    if [[ "$scenario" == "-h" || "$scenario" == "--help" ]]; then
        show_usage
        exit 0
    fi
    
    print_status "WRFCoin K6 Load Testing Suite"
    print_status "Scenario: $scenario"
    print_status "Environment: $environment"
    echo "================================"
    
    # Setup trap for cleanup
    trap cleanup EXIT
    
    # Execute main workflow
    check_prerequisites
    setup_environment "$environment"
    check_service_health
    start_monitoring
    
    if [ "$scenario" = "all" ]; then
        run_all_scenarios
    else
        run_scenario "$scenario"
    fi
    
    generate_report
    
    print_success "Load testing completed!"
    print_status "View results in: $REPORTS_DIR"
    print_status "Grafana dashboard: http://localhost:3004"
}

# Execute main function with all arguments
main "$@"