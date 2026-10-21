#!/bin/bash

# Cross-Platform Validation Script for WRFCoin Dev Container
# This script validates platform-specific features and configurations

set -e

echo "< Cross-Platform Validation for WRFCoin Dev Container"
echo "====================================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
NC='\033[0m' # No Color

# Validation results
VALIDATIONS_PASSED=0
VALIDATIONS_FAILED=0
VALIDATIONS_WARNINGS=0

log_info() {
    echo -e "${BLUE}9  $1${NC}"
}

log_success() {
    echo -e "${GREEN} $1${NC}"
    ((VALIDATIONS_PASSED++))
}

log_warning() {
    echo -e "${YELLOW}   $1${NC}"
    ((VALIDATIONS_WARNINGS++))
}

log_error() {
    echo -e "${RED}L $1${NC}"
    ((VALIDATIONS_FAILED++))
}

log_header() {
    echo -e "${PURPLE}$1${NC}"
}

# Platform-specific validation functions

validate_windows() {
    log_header ">Ÿ Validating Windows-specific features..."
    
    # Check WSL 2 integration
    if [[ -n "$WSL_DISTRO_NAME" ]]; then
        log_success "Running in WSL 2: $WSL_DISTRO_NAME"
        
        # Check WSL version
        if command -v wsl.exe &> /dev/null; then
            WSL_VERSION=$(wsl.exe -l -v 2>/dev/null | grep "$WSL_DISTRO_NAME" | awk '{print $3}' || echo "Unknown")
            if [[ "$WSL_VERSION" == "2" ]]; then
                log_success "WSL version 2 confirmed"
            else
                log_warning "WSL version: $WSL_VERSION (version 2 recommended)"
            fi
        else
            log_warning "Cannot determine WSL version"
        fi
        
        # Check WSL kernel version
        if [[ -f /proc/version ]]; then
            WSL_KERNEL=$(grep -oP "Linux version \K[^\s]+" /proc/version 2>/dev/null)
            if [[ -n "$WSL_KERNEL" ]]; then
                log_info "WSL Kernel: $WSL_KERNEL"
                # Check if kernel supports required features
                if [[ "$WSL_KERNEL" > "5.4" ]]; then
                    log_success "WSL kernel version supports Docker requirements"
                else
                    log_warning "WSL kernel version may be outdated"
                fi
            fi
        fi
    else
        log_error "Not running in WSL 2 environment"
        return 1
    fi
    
    # Check Docker Desktop integration
    if docker info 2>/dev/null | grep -q "Operating System.*Docker Desktop"; then
        log_success "Docker Desktop integration detected"
    else
        log_warning "Docker Desktop integration not clearly detected"
    fi
    
    # Check file system performance
    log_info "Testing file system performance..."
    time_start=$(date +%s%N)
    find /workspace -name "*.js" -type f | head -100 > /dev/null 2>&1 || true
    time_end=$(date +%s%N)
    file_search_time=$(((time_end - time_start) / 1000000))
    
    log_info "File search time: ${file_search_time}ms"
    if [ $file_search_time -lt 2000 ]; then
        log_success "File system performance is acceptable"
    elif [ $file_search_time -lt 5000 ]; then
        log_warning "File system performance is moderate (${file_search_time}ms)"
    else
        log_warning "File system performance may be slow (${file_search_time}ms)"
    fi
    
    # Check Windows-specific environment variables
    if [[ -n "$USERPROFILE" ]] || [[ -n "$PROGRAMFILES" ]]; then
        log_info "Windows environment variables detected"
    fi
    
    # Check for Windows line ending issues
    if command -v git &> /dev/null; then
        autocrlf_setting=$(git config --global core.autocrlf 2>/dev/null || echo "not set")
        log_info "Git autocrlf setting: $autocrlf_setting"
        if [[ "$autocrlf_setting" == "true" || "$autocrlf_setting" == "input" ]]; then
            log_success "Git line ending configuration is appropriate"
        else
            log_warning "Consider setting git config --global core.autocrlf input"
        fi
    fi
}

validate_macos() {
    log_header "<N Validating macOS-specific features..."
    
    # Check architecture
    arch=$(uname -m)
    log_info "Architecture: $arch"
    
    if [[ "$arch" == "arm64" ]]; then
        log_success "Apple Silicon detected"
        
        # Check Rosetta 2 compatibility for x86_64 containers
        if docker run --rm --platform linux/amd64 alpine uname -m 2>/dev/null | grep -q "x86_64"; then
            log_success "x86_64 container emulation working via Rosetta 2"
        else
            log_warning "x86_64 container emulation may not be available"
        fi
        
        # Check for native ARM images
        if docker run --rm --platform linux/arm64 alpine uname -m 2>/dev/null | grep -q "aarch64"; then
            log_success "ARM64 native containers working"
        else
            log_warning "ARM64 native containers may not be available"
        fi
    elif [[ "$arch" == "x86_64" ]]; then
        log_success "Intel Mac detected"
    else
        log_warning "Unknown Mac architecture: $arch"
    fi
    
    # Check macOS version
    if command -v sw_vers &> /dev/null; then
        macos_version=$(sw_vers -productVersion)
        log_info "macOS version: $macos_version"
        
        # Parse version and check compatibility
        major_version=$(echo "$macos_version" | cut -d. -f1)
        minor_version=$(echo "$macos_version" | cut -d. -f2)
        
        if [[ $major_version -gt 11 ]] || [[ $major_version -eq 11 && $minor_version -ge 0 ]]; then
            log_success "macOS version supports Docker Desktop requirements"
        elif [[ $major_version -eq 10 && $minor_version -ge 15 ]]; then
            log_warning "macOS version is supported but updating recommended"
        else
            log_error "macOS version may be too old for optimal Docker Desktop support"
        fi
    fi
    
    # Check Docker Desktop configuration
    if docker info 2>/dev/null | grep -q "File Sharing"; then
        log_success "File sharing is configured"
    else
        log_warning "File sharing configuration may need attention"
    fi
    
    # Check resource allocation
    memory_limit=$(docker info 2>/dev/null | grep "Total Memory" | awk '{print $3 $4}' || echo "Unknown")
    cpu_limit=$(docker info 2>/dev/null | grep "CPUs" | awk '{print $2}' || echo "Unknown")
    
    log_info "Docker resource allocation:"
    log_info "  Memory: $memory_limit"
    log_info "  CPUs: $cpu_limit"
    
    # Check for sufficient resources
    if [[ "$memory_limit" =~ ([0-9.]+)GiB ]]; then
        memory_gb=${BASH_REMATCH[1]}
        if (( $(echo "$memory_gb >= 8" | bc -l) )); then
            log_success "Sufficient memory allocated to Docker"
        else
            log_warning "Docker memory allocation may be insufficient ($memory_limit)"
        fi
    fi
    
    # Check for Homebrew
    if command -v brew &> /dev/null; then
        brew_version=$(brew --version 2>/dev/null | head -1)
        log_success "Homebrew available: $brew_version"
    else
        log_info "Homebrew not installed (optional)"
    fi
    
    # Check file system type
    filesystem=$(df -T /workspace 2>/dev/null | tail -1 | awk '{print $2}' || echo "unknown")
    log_info "Workspace filesystem: $filesystem"
}

validate_linux() {
    log_header "=' Validating Linux-specific features..."
    
    # Check distribution
    if command -v lsb_release &> /dev/null; then
        distro=$(lsb_release -d | cut -f2)
        log_info "Distribution: $distro"
    elif [[ -f /etc/os-release ]]; then
        distro=$(grep "PRETTY_NAME" /etc/os-release | cut -d'"' -f2)
        log_info "Distribution: $distro"
    else
        log_warning "Cannot determine Linux distribution"
    fi
    
    # Check Docker installation type
    if systemctl is-active docker &> /dev/null; then
        log_success "Docker service is active"
        
        # Check if Docker is enabled to start on boot
        if systemctl is-enabled docker &> /dev/null; then
            log_success "Docker service is enabled for startup"
        else
            log_warning "Docker service is not enabled for automatic startup"
        fi
    elif service docker status &> /dev/null; then
        log_success "Docker service is running (non-systemd)"
    else
        log_error "Docker service is not running"
        return 1
    fi
    
    # Check user permissions
    if groups | grep -q docker; then
        log_success "User is in docker group"
    else
        log_error "User is not in docker group"
        log_info "Run: sudo usermod -aG docker \$USER && newgrp docker"
        return 1
    fi
    
    # Check Docker socket permissions
    if [[ -S /var/run/docker.sock ]]; then
        socket_perms=$(ls -la /var/run/docker.sock | awk '{print $1}')
        log_info "Docker socket permissions: $socket_perms"
        
        if [[ "$socket_perms" =~ rw.*rw ]]; then
            log_success "Docker socket is accessible"
        else
            log_warning "Docker socket permissions may be restrictive"
        fi
    else
        log_error "Docker socket not found"
    fi
    
    # Check cgroup version
    if [[ -f /sys/fs/cgroup/cgroup.controllers ]]; then
        log_success "Using cgroup v2"
        
        # Check available controllers
        controllers=$(cat /sys/fs/cgroup/cgroup.controllers 2>/dev/null || echo "")
        if [[ "$controllers" =~ memory ]] && [[ "$controllers" =~ cpu ]]; then
            log_success "Required cgroup controllers available"
        else
            log_warning "Some cgroup controllers may not be available"
        fi
    elif [[ -d /sys/fs/cgroup/systemd ]]; then
        log_info "Using cgroup v1"
    else
        log_warning "Cannot determine cgroup version"
    fi
    
    # Check system limits
    ulimit_nofile=$(ulimit -n)
    ulimit_nproc=$(ulimit -u)
    
    log_info "System limits:"
    log_info "  File descriptors: $ulimit_nofile"
    log_info "  Processes: $ulimit_nproc"
    
    if [ $ulimit_nofile -ge 4096 ]; then
        log_success "File descriptor limit is adequate"
    else
        log_warning "File descriptor limit may be too low ($ulimit_nofile)"
    fi
    
    if [ $ulimit_nproc -ge 2048 ]; then
        log_success "Process limit is adequate"
    else
        log_warning "Process limit may be too low ($ulimit_nproc)"
    fi
    
    # Check for AppArmor/SELinux
    if command -v getenforce &> /dev/null; then
        selinux_status=$(getenforce 2>/dev/null || echo "unknown")
        log_info "SELinux status: $selinux_status"
        if [[ "$selinux_status" == "Enforcing" ]]; then
            log_warning "SELinux is enforcing - may require additional configuration"
        fi
    fi
    
    if command -v aa-status &> /dev/null; then
        if aa-status &> /dev/null; then
            log_info "AppArmor is active"
        fi
    fi
    
    # Check kernel version
    kernel_version=$(uname -r)
    log_info "Kernel version: $kernel_version"
    
    # Check if kernel supports Docker requirements
    if [[ -f /proc/config.gz ]]; then
        if zcat /proc/config.gz | grep -q "CONFIG_NAMESPACES=y"; then
            log_success "Kernel supports namespaces"
        fi
    fi
}

validate_common() {
    log_header "=' Validating common features..."
    
    # Check container runtime
    if docker info 2>/dev/null | grep -q "Runtimes.*runc"; then
        log_success "runc runtime available"
    else
        log_warning "runc runtime not detected"
    fi
    
    # Check containerd runtime
    if docker info 2>/dev/null | grep -q "containerd"; then
        log_success "containerd runtime detected"
    else
        log_info "containerd runtime information not available"
    fi
    
    # Check BuildKit support
    if docker info 2>/dev/null | grep -q "BuildKit"; then
        log_success "BuildKit support detected"
    else
        log_warning "BuildKit support not detected"
        log_info "Consider enabling BuildKit for improved build performance"
    fi
    
    # Check experimental features
    if docker version 2>/dev/null | grep -q "Experimental.*true"; then
        log_info "Docker experimental features enabled"
    fi
    
    # Check volume mounting
    log_info "Testing volume mounting..."
    test_volume_name="wrfcoin-test-volume-$(date +%s)"
    if docker volume create "$test_volume_name" > /dev/null 2>&1; then
        if docker run --rm -v "$test_volume_name:/test" alpine touch /test/testfile > /dev/null 2>&1; then
            log_success "Volume mounting works"
        else
            log_error "Volume mounting failed"
        fi
        docker volume rm "$test_volume_name" > /dev/null 2>&1
    else
        log_error "Volume creation failed"
    fi
    
    # Check bind mounting
    log_info "Testing bind mounting..."
    temp_dir=$(mktemp -d)
    echo "test" > "$temp_dir/testfile"
    if docker run --rm -v "$temp_dir:/test" alpine cat /test/testfile > /dev/null 2>&1; then
        log_success "Bind mounting works"
    else
        log_error "Bind mounting failed"
    fi
    rm -rf "$temp_dir"
    
    # Check network connectivity
    log_info "Testing network connectivity..."
    if docker run --rm alpine ping -c 1 google.com > /dev/null 2>&1; then
        log_success "External network connectivity works"
    elif docker run --rm alpine ping -c 1 8.8.8.8 > /dev/null 2>&1; then
        log_success "External network connectivity works (DNS may have issues)"
    else
        log_error "External network connectivity failed"
    fi
    
    # Check internal networking
    if docker network ls | grep -q bridge; then
        log_success "Docker bridge network available"
    else
        log_warning "Docker bridge network not found"
    fi
    
    # Check port binding
    log_info "Testing port binding..."
    test_container=$(docker run -d -p 19999:80 nginx:alpine 2>/dev/null || echo "failed")
    if [[ "$test_container" != "failed" ]]; then
        sleep 3
        if nc -z localhost 19999 2>/dev/null; then
            log_success "Port binding works"
        else
            log_warning "Port binding may have issues"
        fi
        docker stop "$test_container" > /dev/null 2>&1
        docker rm "$test_container" > /dev/null 2>&1
    else
        log_warning "Port binding test failed to start container"
    fi
    
    # Check Docker Compose functionality
    if command -v docker-compose &> /dev/null; then
        compose_version=$(docker-compose --version)
        log_success "Docker Compose available: $compose_version"
    elif docker compose version &> /dev/null; then
        compose_version=$(docker compose version --short)
        log_success "Docker Compose (plugin) available: v$compose_version"
    else
        log_error "Docker Compose not available"
    fi
    
    # Check image pulling
    log_info "Testing image pulling..."
    if docker pull alpine:latest > /dev/null 2>&1; then
        log_success "Image pulling works"
        docker rmi alpine:latest > /dev/null 2>&1 || true
    else
        log_error "Image pulling failed"
    fi
}

# Main validation logic
main() {
    log_info "Starting cross-platform validation..."
    
    # Detect platform and run appropriate validations
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        if [[ -n "$WSL_DISTRO_NAME" ]]; then
            validate_windows
        else
            validate_linux
        fi
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        validate_macos
    else
        log_warning "Unknown platform: $OSTYPE"
        log_info "Proceeding with common validation only..."
    fi
    
    # Always run common validations
    validate_common
    
    # Summary
    echo ""
    log_header "=Ê Validation Summary"
    log_header "===================="
    
    total_checks=$((VALIDATIONS_PASSED + VALIDATIONS_FAILED + VALIDATIONS_WARNINGS))
    
    echo "Platform: $OSTYPE"
    echo "Total Checks: $total_checks"
    echo "Passed: $VALIDATIONS_PASSED"
    echo "Warnings: $VALIDATIONS_WARNINGS"
    echo "Failed: $VALIDATIONS_FAILED"
    
    if [ $VALIDATIONS_FAILED -eq 0 ]; then
        if [ $VALIDATIONS_WARNINGS -eq 0 ]; then
            log_success "<‰ All validations passed! Platform is ready for development."
            return 0
        else
            log_warning " Validation completed with $VALIDATIONS_WARNINGS warnings."
            log_info "Platform is usable but consider addressing warnings for optimal experience."
            return 0
        fi
    else
        log_error "L Validation completed with $VALIDATIONS_FAILED failures."
        log_info "Platform may not be suitable for development until issues are resolved."
        return 1
    fi
}

# Run validation
main "$@"