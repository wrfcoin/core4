#!/bin/bash

# Comprehensive WRFCoin Dev Container Testing Suite
# This script runs automated tests to validate container functionality across platforms

set -e

echo ">ê WRFCoin Dev Container Test Suite"
echo "===================================="

# Source platform detection
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/detect-platform.sh"

# Create results directory with timestamp
RESULTS_DIR="test-results/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

# Test results tracking
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_TOTAL=0
CRITICAL_FAILURES=()

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}9  $1${NC}"
}

log_success() {
    echo -e "${GREEN} $1${NC}"
}

log_warning() {
    echo -e "${YELLOW}   $1${NC}"
}

log_error() {
    echo -e "${RED}L $1${NC}"
}

run_test() {
    local test_name="$1"
    local test_command="$2"
    local is_critical="${3:-false}"
    local timeout_duration="${4:-300}"
    
    echo ""
    log_info "Running: $test_name"
    ((TESTS_TOTAL++))
    
    local log_file="$RESULTS_DIR/${test_name// /_}.log"
    
    # Run test with timeout
    if timeout "$timeout_duration" bash -c "$test_command" > "$log_file" 2>&1; then
        log_success "PASSED: $test_name"
        ((TESTS_PASSED++))
        return 0
    else
        local exit_code=$?
        log_error "FAILED: $test_name (exit code: $exit_code)"
        ((TESTS_FAILED++))
        
        # Show last few lines of log for debugging
        echo "  Last 5 lines of output:"
        tail -5 "$log_file" | sed 's/^/    /'
        
        if [ "$is_critical" = "true" ]; then
            CRITICAL_FAILURES+=("$test_name")
            log_error "Critical test failed. Continuing with remaining tests..."
        fi
        return 1
    fi
}

cleanup_containers() {
    log_info "Cleaning up test containers..."
    docker-compose -f .devcontainer/docker-compose.dev.yml down --volumes 2>/dev/null || true
    docker rm -f wrfcoin-devcontainer wrfcoin-test-workspace 2>/dev/null || true
    docker rmi wrfcoin-test-build 2>/dev/null || true
}

# Trap to ensure cleanup on exit
trap cleanup_containers EXIT

log_info "Starting test suite for platform: $DETECTED_PLATFORM"
log_info "Results will be saved to: $RESULTS_DIR"

# Test 1: Docker Availability
run_test "Docker Availability" "docker --version && docker info" true 60

# Test 2: Docker Compose Availability
run_test "Docker Compose Availability" "docker-compose --version" true 30

# Test 3: Container Build Test
log_info "Building test container (this may take several minutes)..."
run_test "Container Build" "docker build -t wrfcoin-test-build .devcontainer/" true 600

# Test 4: Docker Compose Configuration Validation
run_test "Docker Compose Config" "docker-compose -f .devcontainer/docker-compose.dev.yml config" false 60

# Test 5: Container Startup
log_info "Starting development services..."
run_test "Container Startup" "docker-compose -f .devcontainer/docker-compose.dev.yml up -d" true 300

# Wait for containers to be ready
log_info "Waiting for containers to initialize..."
sleep 45

# Test 6: Container Health Check
run_test "Container Health" "docker exec wrfcoin-devcontainer-workspace-1 echo 'Container is responsive'" true 30

# Test 7: Service Connectivity Tests
run_test "PostgreSQL Service" "docker exec wrfcoin-devcontainer-workspace-1 timeout 10 bash -c 'until nc -z test-db 5432; do sleep 1; done'" false 60

run_test "Redis Service" "docker exec wrfcoin-devcontainer-workspace-1 timeout 10 bash -c 'until nc -z test-redis 6379; do sleep 1; done'" false 60

run_test "IPFS Service" "docker exec wrfcoin-devcontainer-workspace-1 timeout 10 bash -c 'until nc -z ipfs-node 5001; do sleep 1; done'" false 60

# Test 8: Development Environment Tests
run_test "Node.js Environment" "docker exec wrfcoin-devcontainer-workspace-1 node --version" true 30

run_test "NPM Environment" "docker exec wrfcoin-devcontainer-workspace-1 npm --version" true 30

run_test "Rust Environment" "docker exec wrfcoin-devcontainer-workspace-1 rustc --version" true 30

run_test "Python Environment" "docker exec wrfcoin-devcontainer-workspace-1 python3 --version" true 30

run_test "Git Environment" "docker exec wrfcoin-devcontainer-workspace-1 git --version" true 30

# Test 9: File System Access
run_test "Workspace Access" "docker exec wrfcoin-devcontainer-workspace-1 ls -la /workspace" true 30

run_test "File Permissions" "docker exec wrfcoin-devcontainer-workspace-1 touch /workspace/test-file && rm /workspace/test-file" false 30

# Test 10: Development Tool Tests
run_test "TypeScript Compiler" "docker exec wrfcoin-devcontainer-workspace-1 npx tsc --version" false 60

run_test "Solidity Compiler" "docker exec wrfcoin-devcontainer-workspace-1 solc --version" false 60

run_test "Hardhat CLI" "docker exec wrfcoin-devcontainer-workspace-1 npx hardhat --version" false 60

# Test 11: Package Installation Test
log_info "Testing package installation (this may take a few minutes)..."
run_test "NPM Install Test" "docker exec wrfcoin-devcontainer-workspace-1 bash -c 'cd /workspace && npm install --silent'" false 300

# Test 12: Build Tests (if npm install succeeded)
if [ $TESTS_FAILED -eq 0 ] || [[ ! " ${CRITICAL_FAILURES[@]} " =~ " NPM Install Test " ]]; then
    run_test "TypeScript Type Check" "docker exec wrfcoin-devcontainer-workspace-1 bash -c 'cd /workspace && npm run type-check'" false 120
    
    run_test "Rust Cargo Check" "docker exec wrfcoin-devcontainer-workspace-1 bash -c 'cd /workspace/blockchain-core && cargo check'" false 180
    
    run_test "Frontend Build Test" "docker exec wrfcoin-devcontainer-workspace-1 bash -c 'cd /workspace/frontend && npm run build'" false 300
    
    run_test "Backend Build Test" "docker exec wrfcoin-devcontainer-workspace-1 bash -c 'cd /workspace/backend && npm run build'" false 300
else
    log_warning "Skipping build tests due to NPM installation failure"
fi

# Test 13: Port Accessibility Tests
run_test "Port 3000 Available" "docker exec wrfcoin-devcontainer-workspace-1 bash -c 'nc -z localhost 3000 || echo \"Port not bound but available\"'" false 30

run_test "Port 4001 Available" "docker exec wrfcoin-devcontainer-workspace-1 bash -c 'nc -z localhost 4001 || echo \"Port not bound but available\"'" false 30

# Performance Tests
echo ""
log_info "=€ Performance Tests"
log_info "==================="

# Test container restart time
log_info "Testing container restart performance..."
restart_start=$(date +%s)
docker-compose -f .devcontainer/docker-compose.dev.yml restart workspace > /dev/null 2>&1
sleep 30  # Wait for services to be ready
restart_end=$(date +%s)
restart_time=$((restart_end - restart_start))

# Test build performance (if applicable)
build_time="N/A"
if [ $TESTS_FAILED -eq 0 ] || [[ ! " ${CRITICAL_FAILURES[@]} " =~ " NPM Install Test " ]]; then
    log_info "Testing build performance..."
    build_start=$(date +%s)
    docker exec wrfcoin-devcontainer-workspace-1 bash -c 'cd /workspace && timeout 300 npm run build --silent' > /dev/null 2>&1 || true
    build_end=$(date +%s)
    build_time=$((build_end - build_start))
fi

# Test memory usage
memory_usage=$(docker stats wrfcoin-devcontainer-workspace-1 --no-stream --format "{{.MemUsage}}" 2>/dev/null | cut -d'/' -f1 || echo "N/A")

# Test CPU usage
cpu_usage=$(docker stats wrfcoin-devcontainer-workspace-1 --no-stream --format "{{.CPUPerc}}" 2>/dev/null || echo "N/A")

# Generate comprehensive test report
cat > "$RESULTS_DIR/test-summary.json" << EOF
{
  "platform": "$DETECTED_PLATFORM",
  "architecture": "$DETECTED_ARCH",
  "timestamp": "$(date -Iseconds)",
  "test_session": {
    "total": $TESTS_TOTAL,
    "passed": $TESTS_PASSED,
    "failed": $TESTS_FAILED,
    "success_rate": $(echo "scale=2; $TESTS_PASSED * 100 / $TESTS_TOTAL" | bc -l 2>/dev/null || echo "0")
  },
  "critical_failures": [$(printf '"%s",' "${CRITICAL_FAILURES[@]}" | sed 's/,$//')]],
  "performance": {
    "restart_time_seconds": $restart_time,
    "build_time_seconds": "$build_time",
    "memory_usage": "$memory_usage",
    "cpu_usage": "$cpu_usage"
  },
  "system_info": {
    "cpu_cores": "$DETECTED_CPU_CORES",
    "total_ram": "$DETECTED_TOTAL_RAM",
    "docker_version": "$(docker --version 2>/dev/null || echo 'N/A')"
  }
}
EOF

# Generate human-readable report
cat > "$RESULTS_DIR/test-report.md" << EOF
# WRFCoin Dev Container Test Report

**Platform**: $DETECTED_PLATFORM  
**Architecture**: $DETECTED_ARCH  
**Date**: $(date)  
**Test Session ID**: $(basename "$RESULTS_DIR")

## Test Summary

- **Total Tests**: $TESTS_TOTAL
- **Passed**: $TESTS_PASSED
- **Failed**: $TESTS_FAILED
- **Success Rate**: $(echo "scale=1; $TESTS_PASSED * 100 / $TESTS_TOTAL" | bc -l 2>/dev/null || echo "0")%

## Critical Failures

$(if [ ${#CRITICAL_FAILURES[@]} -eq 0 ]; then
    echo "None <‰"
else
    printf "- %s\n" "${CRITICAL_FAILURES[@]}"
fi)

## Performance Metrics

- **Container Restart Time**: ${restart_time}s
- **Build Time**: ${build_time}s
- **Memory Usage**: $memory_usage
- **CPU Usage**: $cpu_usage

## System Information

- **CPU Cores**: $DETECTED_CPU_CORES
- **Total RAM**: $DETECTED_TOTAL_RAM
- **Docker Version**: $(docker --version 2>/dev/null || echo 'N/A')

## Recommendations

$(if [ $TESTS_FAILED -eq 0 ]; then
    echo " All tests passed! The development container is ready for use."
elif [ ${#CRITICAL_FAILURES[@]} -eq 0 ]; then
    echo "   Some non-critical tests failed. The container is usable but may have minor issues."
    echo "Check individual test logs in the results directory for details."
else
    echo "L Critical failures detected. The development container may not be suitable for development."
    echo "Review and resolve critical issues before proceeding."
fi)

EOF

# Final summary output
echo ""
log_info "=Ê Test Summary"
log_info "==============="
echo "Platform: $DETECTED_PLATFORM"
echo "Tests Total: $TESTS_TOTAL"
echo "Tests Passed: $TESTS_PASSED"
echo "Tests Failed: $TESTS_FAILED"
echo "Success Rate: $(echo "scale=1; $TESTS_PASSED * 100 / $TESTS_TOTAL" | bc -l 2>/dev/null || echo "0")%"

if [ ${#CRITICAL_FAILURES[@]} -gt 0 ]; then
    echo ""
    log_error "Critical Failures:"
    printf "  - %s\n" "${CRITICAL_FAILURES[@]}"
fi

echo ""
log_info "Performance:"
echo "  Restart Time: ${restart_time}s"
echo "  Build Time: ${build_time}s"
echo "  Memory Usage: $memory_usage"
echo "  CPU Usage: $cpu_usage"

echo ""
log_info "Results saved to: $RESULTS_DIR"
echo "  - test-summary.json (machine-readable)"
echo "  - test-report.md (human-readable)"
echo "  - Individual test logs"

# Determine exit code
if [ ${#CRITICAL_FAILURES[@]} -gt 0 ]; then
    log_error "Test suite completed with critical failures"
    exit 2
elif [ $TESTS_FAILED -gt 0 ]; then
    log_warning "Test suite completed with non-critical failures"
    exit 1
else
    log_success "All tests passed! <‰"
    exit 0
fi