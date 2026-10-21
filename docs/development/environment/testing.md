# WRFCoin Dev Container Platform Testing Checklist

This document provides a comprehensive testing checklist for validating the WRFCoin development
container across Windows, macOS, and Linux platforms.

## 🎯 Testing Objectives

- Verify dev container functionality across all major platforms
- Ensure consistent development experience
- Identify and document platform-specific issues
- Validate performance characteristics
- Test enterprise deployment scenarios

## 📋 Pre-Testing Requirements

### Test Environment Setup

#### Minimum Test Systems

- **Windows 11** with WSL 2 (latest)
- **macOS Monterey** (12.0+) with Apple Silicon and Intel
- **Ubuntu 22.04 LTS** (latest)

#### Required Tools

- Docker Desktop (latest stable version)
- Visual Studio Code (latest)
- Git (latest)
- Internet connection for downloading dependencies

#### Test Data

- Clean git clone of WRFCoin repository
- Sample API keys (for testing external integrations)
- Test database fixtures

## 🧪 Automated Testing Scripts

### Platform Detection Script

Create `scripts/detect-platform.sh`:

```bash
#!/bin/bash

# Detect platform and system specifications
echo "🖥️  Platform Detection Report"
echo "============================"

# Operating System
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    PLATFORM="Linux"
    DISTRO=$(lsb_release -d | cut -f2)
    echo "Platform: $PLATFORM ($DISTRO)"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    PLATFORM="macOS"
    VERSION=$(sw_vers -productVersion)
    ARCH=$(uname -m)
    echo "Platform: $PLATFORM $VERSION ($ARCH)"
elif [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "win32" ]]; then
    PLATFORM="Windows"
    echo "Platform: $PLATFORM (detected via $OSTYPE)"
else
    PLATFORM="Unknown"
    echo "Platform: $PLATFORM ($OSTYPE)"
fi

# System Resources
echo ""
echo "System Resources:"
echo "  CPU Cores: $(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 'N/A')"
echo "  Total RAM: $(free -h 2>/dev/null | awk '/^Mem:/ {print $2}' || sysctl -n hw.memsize 2>/dev/null | awk '{print $1/1024/1024/1024 " GB"}' || echo 'N/A')"
echo "  Available Disk: $(df -h . | tail -1 | awk '{print $4}')"

# Docker Information
echo ""
echo "Docker Environment:"
if command -v docker &> /dev/null; then
    echo "  Docker Version: $(docker --version)"
    echo "  Docker Compose: $(docker-compose --version 2>/dev/null || echo 'Not available')"
    echo "  Docker Status: $(docker info >/dev/null 2>&1 && echo 'Running' || echo 'Not running')"
else
    echo "  Docker: Not installed"
fi

# VS Code Information
echo ""
echo "Development Tools:"
if command -v code &> /dev/null; then
    echo "  VS Code: $(code --version | head -1)"
else
    echo "  VS Code: Not in PATH"
fi

echo "  Git: $(git --version 2>/dev/null || echo 'Not available')"
echo "  Node.js: $(node --version 2>/dev/null || echo 'Not available')"
echo "  Python: $(python3 --version 2>/dev/null || echo 'Not available')"

export DETECTED_PLATFORM="$PLATFORM"
```

### Container Testing Suite

Create `scripts/test-container.sh`:

```bash
#!/bin/bash

# Comprehensive container testing suite
set -e

echo "🧪 WRFCoin Dev Container Test Suite"
echo "==================================="

# Source platform detection
source scripts/detect-platform.sh

RESULTS_DIR="test-results/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

# Test results tracking
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_TOTAL=0

run_test() {
    local test_name="$1"
    local test_command="$2"
    local is_critical="${3:-false}"

    echo ""
    echo "▶️  Running: $test_name"
    ((TESTS_TOTAL++))

    if eval "$test_command" > "$RESULTS_DIR/${test_name// /_}.log" 2>&1; then
        echo "✅ PASSED: $test_name"
        ((TESTS_PASSED++))
        return 0
    else
        echo "❌ FAILED: $test_name"
        ((TESTS_FAILED++))

        if [ "$is_critical" = "true" ]; then
            echo "💥 Critical test failed. Stopping test suite."
            cat "$RESULTS_DIR/${test_name// /_}.log"
            exit 1
        fi
        return 1
    fi
}

# Test 1: Docker Availability
run_test "Docker Availability" "docker --version && docker info" true

# Test 2: Container Build
run_test "Container Build" "docker build -t wrfcoin-test-build .devcontainer/" true

# Test 3: Container Startup
run_test "Container Startup" "docker-compose -f .devcontainer/docker-compose.dev.yml up -d workspace"

# Wait for container to be ready
echo "⏳ Waiting for container to be ready..."
sleep 30

# Test 4: Container Health
run_test "Container Health" "docker exec wrfcoin-devcontainer echo 'Container is responsive'"

# Test 5: Node.js Environment
run_test "Node.js Environment" "docker exec wrfcoin-devcontainer node --version"

# Test 6: NPM Dependencies
run_test "NPM Dependencies" "docker exec wrfcoin-devcontainer npm --version"

# Test 7: Rust Environment
run_test "Rust Environment" "docker exec wrfcoin-devcontainer rustc --version"

# Test 8: Python Environment
run_test "Python Environment" "docker exec wrfcoin-devcontainer python3 --version"

# Test 9: Database Connection
run_test "Database Connection" "docker exec wrfcoin-devcontainer nc -z test-db 5432"

# Test 10: Redis Connection
run_test "Redis Connection" "docker exec wrfcoin-devcontainer nc -z test-redis 6379"

# Test 11: File System Access
run_test "File System Access" "docker exec wrfcoin-devcontainer ls -la /workspace"

# Test 12: Git Operations
run_test "Git Operations" "docker exec wrfcoin-devcontainer git status"

# Test 13: NPM Install
run_test "NPM Install" "docker exec wrfcoin-devcontainer npm install --silent"

# Test 14: TypeScript Compilation
run_test "TypeScript Compilation" "docker exec wrfcoin-devcontainer npm run type-check"

# Test 15: Rust Compilation
run_test "Rust Compilation" "docker exec wrfcoin-devcontainer bash -c 'cd blockchain-core && cargo check'"

# Test 16: Smart Contract Compilation
run_test "Smart Contract Compilation" "docker exec wrfcoin-devcontainer bash -c 'cd contracts && npx hardhat compile'"

# Test 17: Frontend Build
run_test "Frontend Build" "docker exec wrfcoin-devcontainer bash -c 'cd frontend && npm run build'"

# Test 18: Backend Build
run_test "Backend Build" "docker exec wrfcoin-devcontainer bash -c 'cd backend && npm run build'"

# Test 19: Documentation Build
run_test "Documentation Build" "docker exec wrfcoin-devcontainer npm run docs:generate"

# Test 20: Port Accessibility
run_test "Port Accessibility" "timeout 10 bash -c 'until nc -z localhost 5432; do sleep 1; done'"

# Performance Tests
echo ""
echo "🚀 Performance Tests"
echo "===================="

# Test startup time
start_time=$(date +%s)
docker-compose -f .devcontainer/docker-compose.dev.yml restart workspace > /dev/null 2>&1
sleep 30  # Wait for services to be ready
end_time=$(date +%s)
startup_time=$((end_time - start_time))

echo "📊 Performance Metrics:"
echo "  Container restart time: ${startup_time}s"

# Test build performance
build_start=$(date +%s)
docker exec wrfcoin-devcontainer npm run build --silent > /dev/null 2>&1
build_end=$(date +%s)
build_time=$((build_end - build_start))
echo "  Full build time: ${build_time}s"

# Test memory usage
memory_usage=$(docker stats wrfcoin-devcontainer --no-stream --format "{{.MemUsage}}" | cut -d'/' -f1)
echo "  Memory usage: $memory_usage"

# Cleanup
echo ""
echo "🧹 Cleaning up test environment..."
docker-compose -f .devcontainer/docker-compose.dev.yml down > /dev/null 2>&1
docker rmi wrfcoin-test-build > /dev/null 2>&1

# Generate test report
cat > "$RESULTS_DIR/test-summary.json" << EOF
{
  "platform": "$DETECTED_PLATFORM",
  "timestamp": "$(date -Iseconds)",
  "tests": {
    "total": $TESTS_TOTAL,
    "passed": $TESTS_PASSED,
    "failed": $TESTS_FAILED,
    "success_rate": $(echo "scale=2; $TESTS_PASSED * 100 / $TESTS_TOTAL" | bc -l)%
  },
  "performance": {
    "startup_time_seconds": $startup_time,
    "build_time_seconds": $build_time,
    "memory_usage": "$memory_usage"
  }
}
EOF

# Final summary
echo ""
echo "📊 Test Summary"
echo "==============="
echo "Platform: $DETECTED_PLATFORM"
echo "Tests Total: $TESTS_TOTAL"
echo "Tests Passed: $TESTS_PASSED"
echo "Tests Failed: $TESTS_FAILED"
echo "Success Rate: $(echo "scale=2; $TESTS_PASSED * 100 / $TESTS_TOTAL" | bc -l)%"
echo ""
echo "Performance:"
echo "  Startup Time: ${startup_time}s"
echo "  Build Time: ${build_time}s"
echo "  Memory Usage: $memory_usage"
echo ""
echo "Results saved to: $RESULTS_DIR"

if [ $TESTS_FAILED -eq 0 ]; then
    echo "🎉 All tests passed!"
    exit 0
else
    echo "⚠️  Some tests failed. Check logs in $RESULTS_DIR"
    exit 1
fi
```

### Cross-Platform Validation Script

Create `scripts/validate-cross-platform.sh`:

```bash
#!/bin/bash

# Cross-platform validation script
echo "🌍 Cross-Platform Validation for WRFCoin Dev Container"
echo "===================================================="

# Platform-specific validation functions

validate_windows() {
    echo "🪟 Validating Windows-specific features..."

    # Check WSL 2 integration
    if [[ -n "$WSL_DISTRO_NAME" ]]; then
        echo "✅ Running in WSL 2: $WSL_DISTRO_NAME"
    else
        echo "❌ Not running in WSL 2"
        return 1
    fi

    # Check Docker Desktop integration
    if docker info | grep -q "Operating System.*Docker Desktop"; then
        echo "✅ Docker Desktop integration detected"
    else
        echo "⚠️  Docker Desktop integration not detected"
    fi

    # Check file system performance
    echo "📁 Testing file system performance..."
    time_start=$(date +%s%N)
    find /workspace -name "*.js" -type f | head -100 > /dev/null
    time_end=$(date +%s%N)
    file_search_time=$(((time_end - time_start) / 1000000))
    echo "  File search time: ${file_search_time}ms"

    if [ $file_search_time -lt 1000 ]; then
        echo "✅ File system performance is good"
    else
        echo "⚠️  File system performance may be slow"
    fi
}

validate_macos() {
    echo "🍎 Validating macOS-specific features..."

    # Check architecture
    arch=$(uname -m)
    echo "🏗️  Architecture: $arch"

    if [[ "$arch" == "arm64" ]]; then
        echo "✅ Apple Silicon detected"
        # Check Rosetta 2 compatibility for x86_64 containers
        if docker run --rm alpine uname -m | grep -q "x86_64"; then
            echo "✅ x86_64 container emulation working"
        else
            echo "⚠️  x86_64 container emulation may not be available"
        fi
    else
        echo "✅ Intel Mac detected"
    fi

    # Check file sharing configuration
    if docker info | grep -q "File Sharing"; then
        echo "✅ File sharing is configured"
    else
        echo "⚠️  File sharing configuration may need attention"
    fi

    # Check resource allocation
    memory_limit=$(docker info | grep "Total Memory" | awk '{print $3 $4}')
    echo "  Available memory: $memory_limit"

    cpu_limit=$(docker info | grep "CPUs" | awk '{print $2}')
    echo "  Available CPUs: $cpu_limit"
}

validate_linux() {
    echo "🐧 Validating Linux-specific features..."

    # Check distribution
    if command -v lsb_release &> /dev/null; then
        distro=$(lsb_release -d | cut -f2)
        echo "🏷️  Distribution: $distro"
    fi

    # Check Docker installation type
    if systemctl is-active docker &> /dev/null; then
        echo "✅ Docker service is active"
    else
        echo "❌ Docker service is not running"
        return 1
    fi

    # Check user permissions
    if groups | grep -q docker; then
        echo "✅ User is in docker group"
    else
        echo "❌ User is not in docker group"
        return 1
    fi

    # Check cgroup version
    if [[ -f /sys/fs/cgroup/cgroup.controllers ]]; then
        echo "✅ Using cgroup v2"
    else
        echo "ℹ️  Using cgroup v1"
    fi

    # Check system limits
    ulimit_nofile=$(ulimit -n)
    echo "  File descriptor limit: $ulimit_nofile"

    if [ $ulimit_nofile -ge 4096 ]; then
        echo "✅ File descriptor limit is adequate"
    else
        echo "⚠️  File descriptor limit may be too low"
    fi
}

validate_common() {
    echo "🔧 Validating common features..."

    # Check container runtime
    if docker info | grep -q "Runtimes.*runc"; then
        echo "✅ runc runtime available"
    else
        echo "⚠️  runc runtime not detected"
    fi

    # Check BuildKit support
    if docker info | grep -q "BuildKit"; then
        echo "✅ BuildKit support detected"
    else
        echo "⚠️  BuildKit support not detected"
    fi

    # Check volume mounting
    if docker run --rm -v "$(pwd):/test" alpine ls /test > /dev/null 2>&1; then
        echo "✅ Volume mounting works"
    else
        echo "❌ Volume mounting failed"
        return 1
    fi

    # Check network connectivity
    if docker run --rm alpine ping -c 1 google.com > /dev/null 2>&1; then
        echo "✅ Network connectivity works"
    else
        echo "❌ Network connectivity failed"
        return 1
    fi

    # Check port binding
    if docker run --rm -d -p 9999:80 nginx > /dev/null 2>&1; then
        container_id=$(docker ps -q --filter "publish=9999")
        if [[ -n "$container_id" ]]; then
            echo "✅ Port binding works"
            docker stop "$container_id" > /dev/null 2>&1
        else
            echo "❌ Port binding failed"
        fi
    else
        echo "❌ Port binding failed"
    fi
}

# Main validation logic
main() {
    # Detect platform
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        if [[ -n "$WSL_DISTRO_NAME" ]]; then
            validate_windows
        else
            validate_linux
        fi
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        validate_macos
    else
        echo "❓ Unknown platform: $OSTYPE"
        echo "Proceeding with common validation only..."
    fi

    validate_common

    echo ""
    echo "🎯 Platform validation complete!"
}

# Run validation
main "$@"
```

## ✅ Manual Testing Checklist

### Windows Testing Checklist

#### Environment Setup

- [ ] Windows 10/11 Pro/Enterprise with WSL 2 enabled
- [ ] Docker Desktop installed with WSL 2 backend
- [ ] VS Code with Remote-Containers extension
- [ ] Git for Windows or Git in WSL

#### Container Functionality

- [ ] Dev container builds successfully
- [ ] All services start without errors
- [ ] Port forwarding works (can access localhost:3000, 4001, etc.)
- [ ] File watching works for hot reload
- [ ] Git operations work from within container
- [ ] Terminal integration works properly

#### Performance Testing

- [ ] Container startup time < 2 minutes
- [ ] Hot reload works within 3 seconds
- [ ] Full build completes within reasonable time
- [ ] Memory usage stays within limits

#### Windows-Specific Issues

- [ ] No permission errors with bind mounts
- [ ] Line ending conversion works correctly
- [ ] WSL 2 resource limits don't cause issues
- [ ] Anti-virus doesn't interfere with container operation

### macOS Testing Checklist

#### Environment Setup

- [ ] macOS 10.15+ (Intel) or 11.0+ (Apple Silicon)
- [ ] Docker Desktop for Mac installed
- [ ] VS Code with Remote-Containers extension
- [ ] Sufficient Docker resource allocation

#### Container Functionality

- [ ] Dev container builds on both Intel and Apple Silicon
- [ ] All services start without errors
- [ ] Port forwarding works correctly
- [ ] File sharing performance is acceptable
- [ ] Volume mounts work correctly

#### Performance Testing

- [ ] Container startup time reasonable
- [ ] File I/O performance acceptable
- [ ] Build times within expected range
- [ ] Resource usage monitored

#### macOS-Specific Issues

- [ ] File sharing doesn't cause performance issues
- [ ] Resource limits properly configured
- [ ] No Docker Desktop crashes or hangs
- [ ] Apple Silicon compatibility verified

### Linux Testing Checklist

#### Environment Setup

- [ ] Ubuntu 20.04+ or equivalent distribution
- [ ] Docker and Docker Compose installed
- [ ] User in docker group
- [ ] VS Code with Remote-Containers extension

#### Container Functionality

- [ ] Dev container builds successfully
- [ ] All services start correctly
- [ ] Networking works as expected
- [ ] File permissions correct
- [ ] Resource access not restricted

#### Performance Testing

- [ ] Native performance (baseline)
- [ ] Container startup time
- [ ] Build performance
- [ ] Memory and CPU usage

#### Linux-Specific Issues

- [ ] No permission issues with Docker socket
- [ ] SELinux/AppArmor compatibility
- [ ] System resource limits adequate
- [ ] cgroup configuration correct

## 📊 Performance Benchmarks

### Expected Performance Targets

| Operation       | Windows (WSL2) | macOS (Intel) | macOS (Apple Silicon) | Linux (Native) |
| --------------- | -------------- | ------------- | --------------------- | -------------- |
| Container Start | < 60s          | < 45s         | < 30s                 | < 30s          |
| Full Build      | < 5min         | < 4min        | < 3min                | < 3min         |
| Hot Reload      | < 5s           | < 3s          | < 2s                  | < 2s           |
| Test Suite      | < 10min        | < 8min        | < 6min                | < 5min         |

### Memory Usage Targets

| Component         | Memory Usage |
| ----------------- | ------------ |
| Base Container    | < 1GB        |
| Development Tools | < 2GB        |
| Build Process     | < 4GB        |
| Total Peak        | < 6GB        |

## 🚨 Known Issues and Workarounds

### Windows Issues

1. **Slow file I/O with bind mounts**

   - Use WSL 2 file system for source code
   - Consider named volumes for node_modules

2. **Port conflicts with Windows services**

   - Check for IIS, SQL Server, or other services
   - Use netstat to identify conflicting processes

3. **WSL 2 memory consumption**
   - Configure .wslconfig to limit memory usage
   - Restart WSL periodically: `wsl --shutdown`

### macOS Issues

1. **Poor file sharing performance**

   - Enable VirtioFS in Docker Desktop
   - Use delegated or cached mount options

2. **Resource allocation**

   - Increase Docker Desktop resource limits
   - Monitor Activity Monitor for resource usage

3. **Apple Silicon compatibility**
   - Some tools may need Rosetta 2
   - Use multi-platform images when possible

### Linux Issues

1. **Permission problems**

   - Ensure user is in docker group
   - Check file ownership in containers

2. **Resource limits**

   - Increase ulimits if needed
   - Monitor systemd resource constraints

3. **Network configuration**
   - Check firewall rules
   - Verify Docker network settings

## 📝 Test Reports and Validation

After completing testing on each platform, document results using this template:

### Test Report Template

```markdown
# WRFCoin Dev Container Test Report

**Date**: [DATE] **Platform**: [Windows 11 WSL2 / macOS 12.6 Intel / Ubuntu 22.04] **Tester**:
[NAME] **Docker Version**: [VERSION] **VS Code Version**: [VERSION]

## Test Results

### Automated Tests

- [ ] All automated tests passed
- [ ] Performance within targets
- [ ] No critical failures

### Manual Validation

- [ ] Container builds successfully
- [ ] All services accessible
- [ ] Development workflow functional
- [ ] Documentation accurate

### Issues Found

1. [Issue description]
   - **Severity**: [High/Medium/Low]
   - **Workaround**: [Description]
   - **Resolution**: [Description or "Pending"]

### Performance Results

- Container startup: [TIME]
- Full build: [TIME]
- Memory usage: [AMOUNT]

### Recommendations

- [Recommendation 1]
- [Recommendation 2]

### Overall Assessment

- [ ] ✅ Approved for production use
- [ ] ⚠️ Approved with known issues
- [ ] ❌ Not approved - blocking issues
```

This comprehensive testing approach ensures the WRFCoin development container works reliably across
all supported platforms and provides a consistent, high-quality development experience for all team
members.
