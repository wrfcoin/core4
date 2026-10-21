#!/bin/bash

# Platform Detection Script for WRFCoin Dev Container Testing
# This script detects the host platform and system specifications

echo "🖥️  Platform Detection Report"
echo "============================"

# Detect Operating System
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    if [[ -n "$WSL_DISTRO_NAME" ]]; then
        PLATFORM="Windows WSL2"
        echo "Platform: $PLATFORM ($WSL_DISTRO_NAME)"
        
        # Get Windows version if available
        if command -v cmd.exe &> /dev/null; then
            WIN_VERSION=$(cmd.exe /c "ver" 2>/dev/null | grep -oP "Version \K[0-9]+\.[0-9]+\.[0-9]+")
            echo "Windows Version: $WIN_VERSION"
        fi
    else
        PLATFORM="Linux"
        # Try to get distribution info
        if command -v lsb_release &> /dev/null; then
            DISTRO=$(lsb_release -d | cut -f2)
        elif [[ -f /etc/os-release ]]; then
            DISTRO=$(grep "PRETTY_NAME" /etc/os-release | cut -d'"' -f2)
        else
            DISTRO="Unknown Linux"
        fi
        echo "Platform: $PLATFORM ($DISTRO)"
    fi
elif [[ "$OSTYPE" == "darwin"* ]]; then
    PLATFORM="macOS"
    VERSION=$(sw_vers -productVersion)
    ARCH=$(uname -m)
    CHIP_INFO="Unknown"
    
    # Detect Apple Silicon vs Intel
    if [[ "$ARCH" == "arm64" ]]; then
        CHIP_INFO="Apple Silicon"
    elif [[ "$ARCH" == "x86_64" ]]; then
        CHIP_INFO="Intel"
    fi
    
    echo "Platform: $PLATFORM $VERSION ($CHIP_INFO)"
elif [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "win32" ]] || [[ "$OSTYPE" == "cygwin" ]]; then
    PLATFORM="Windows Native"
    echo "Platform: $PLATFORM (detected via $OSTYPE)"
else
    PLATFORM="Unknown"
    echo "Platform: $PLATFORM ($OSTYPE)"
fi

# System Resources
echo ""
echo "System Resources:"

# CPU Information
if command -v nproc &> /dev/null; then
    CPU_CORES=$(nproc)
elif command -v sysctl &> /dev/null; then
    CPU_CORES=$(sysctl -n hw.ncpu 2>/dev/null || echo "N/A")
else
    CPU_CORES="N/A"
fi
echo "  CPU Cores: $CPU_CORES"

# Memory Information
if command -v free &> /dev/null; then
    TOTAL_RAM=$(free -h | awk '/^Mem:/ {print $2}')
    AVAILABLE_RAM=$(free -h | awk '/^Mem:/ {print $7}')
    echo "  Total RAM: $TOTAL_RAM"
    echo "  Available RAM: $AVAILABLE_RAM"
elif command -v sysctl &> /dev/null; then
    TOTAL_RAM_BYTES=$(sysctl -n hw.memsize 2>/dev/null)
    if [[ -n "$TOTAL_RAM_BYTES" ]]; then
        TOTAL_RAM=$(echo "$TOTAL_RAM_BYTES" | awk '{printf "%.1f GB", $1/1024/1024/1024}')
        echo "  Total RAM: $TOTAL_RAM"
    else
        echo "  Total RAM: N/A"
    fi
else
    echo "  RAM: N/A"
fi

# Disk Space
AVAILABLE_DISK=$(df -h . | tail -1 | awk '{print $4}')
DISK_USAGE=$(df -h . | tail -1 | awk '{print $5}')
echo "  Available Disk: $AVAILABLE_DISK"
echo "  Disk Usage: $DISK_USAGE"

# Docker Information
echo ""
echo "Docker Environment:"
if command -v docker &> /dev/null; then
    DOCKER_VERSION=$(docker --version 2>/dev/null | cut -d',' -f1)
    echo "  Docker Version: $DOCKER_VERSION"
    
    # Docker Compose
    if command -v docker-compose &> /dev/null; then
        COMPOSE_VERSION=$(docker-compose --version 2>/dev/null)
        echo "  Docker Compose: $COMPOSE_VERSION"
    elif docker compose version &> /dev/null; then
        COMPOSE_VERSION=$(docker compose version --short 2>/dev/null)
        echo "  Docker Compose: v$COMPOSE_VERSION (plugin)"
    else
        echo "  Docker Compose: Not available"
    fi
    
    # Docker Status
    if docker info >/dev/null 2>&1; then
        echo "  Docker Status: Running"
        
        # Additional Docker info
        DOCKER_ROOT_DIR=$(docker info 2>/dev/null | grep "Docker Root Dir" | cut -d':' -f2 | xargs)
        if [[ -n "$DOCKER_ROOT_DIR" ]]; then
            echo "  Docker Root: $DOCKER_ROOT_DIR"
        fi
        
        # Get Docker engine info
        DOCKER_ENGINE=$(docker info 2>/dev/null | grep "Server Version" | cut -d':' -f2 | xargs)
        if [[ -n "$DOCKER_ENGINE" ]]; then
            echo "  Engine Version: $DOCKER_ENGINE"
        fi
        
        # Check for BuildKit
        if docker info 2>/dev/null | grep -q "BuildKit"; then
            echo "  BuildKit: Enabled"
        else
            echo "  BuildKit: Disabled"
        fi
        
    else
        echo "  Docker Status: Not running"
    fi
else
    echo "  Docker: Not installed"
fi

# Development Tools
echo ""
echo "Development Tools:"

# VS Code
if command -v code &> /dev/null; then
    CODE_VERSION=$(code --version 2>/dev/null | head -1)
    echo "  VS Code: $CODE_VERSION"
    
    # Check for Remote-Containers extension
    if code --list-extensions 2>/dev/null | grep -q "ms-vscode-remote.remote-containers"; then
        echo "  Remote-Containers: Installed"
    else
        echo "  Remote-Containers: Not installed"
    fi
else
    echo "  VS Code: Not in PATH"
fi

# Git
if command -v git &> /dev/null; then
    GIT_VERSION=$(git --version 2>/dev/null)
    echo "  Git: $GIT_VERSION"
else
    echo "  Git: Not available"
fi

# Node.js
if command -v node &> /dev/null; then
    NODE_VERSION=$(node --version 2>/dev/null)
    echo "  Node.js: $NODE_VERSION"
else
    echo "  Node.js: Not available"
fi

# Python
if command -v python3 &> /dev/null; then
    PYTHON_VERSION=$(python3 --version 2>/dev/null)
    echo "  Python: $PYTHON_VERSION"
elif command -v python &> /dev/null; then
    PYTHON_VERSION=$(python --version 2>/dev/null)
    echo "  Python: $PYTHON_VERSION"
else
    echo "  Python: Not available"
fi

# Rust
if command -v rustc &> /dev/null; then
    RUST_VERSION=$(rustc --version 2>/dev/null)
    echo "  Rust: $RUST_VERSION"
else
    echo "  Rust: Not available"
fi

# Platform-Specific Information
echo ""
echo "Platform-Specific Information:"

if [[ "$PLATFORM" == "Windows WSL2" ]]; then
    # WSL-specific info
    if [[ -f /proc/version ]]; then
        WSL_KERNEL=$(grep -oP "Linux version \K[^\s]+" /proc/version 2>/dev/null)
        echo "  WSL Kernel: $WSL_KERNEL"
    fi
    
    # Check WSL version
    if command -v wsl.exe &> /dev/null; then
        WSL_VERSION=$(wsl.exe -l -v 2>/dev/null | grep "$WSL_DISTRO_NAME" | awk '{print $3}' || echo "Unknown")
        echo "  WSL Version: $WSL_VERSION"
    fi
    
elif [[ "$PLATFORM" == "macOS"* ]]; then
    # macOS-specific info
    echo "  Architecture: $ARCH"
    
    # Check for Homebrew
    if command -v brew &> /dev/null; then
        BREW_VERSION=$(brew --version 2>/dev/null | head -1)
        echo "  Homebrew: $BREW_VERSION"
    else
        echo "  Homebrew: Not installed"
    fi
    
elif [[ "$PLATFORM" == "Linux" ]]; then
    # Linux-specific info
    if command -v systemctl &> /dev/null; then
        echo "  Init System: systemd"
        
        # Check Docker service status
        if systemctl is-active docker &> /dev/null; then
            echo "  Docker Service: Active"
        else
            echo "  Docker Service: Inactive"
        fi
    else
        echo "  Init System: Non-systemd"
    fi
    
    # Check user groups
    if groups | grep -q docker; then
        echo "  Docker Group: User is member"
    else
        echo "  Docker Group: User not in group"
    fi
    
    # Check cgroup version
    if [[ -f /sys/fs/cgroup/cgroup.controllers ]]; then
        echo "  CGroup: v2"
    elif [[ -d /sys/fs/cgroup/systemd ]]; then
        echo "  CGroup: v1"
    else
        echo "  CGroup: Unknown"
    fi
fi

# Network Information
echo ""
echo "Network Information:"

# Check internet connectivity
if ping -c 1 google.com &> /dev/null || ping -c 1 8.8.8.8 &> /dev/null; then
    echo "  Internet: Connected"
else
    echo "  Internet: Not connected"
fi

# Check for proxy settings
if [[ -n "$HTTP_PROXY" ]] || [[ -n "$HTTPS_PROXY" ]] || [[ -n "$http_proxy" ]] || [[ -n "$https_proxy" ]]; then
    echo "  Proxy: Configured"
    [[ -n "$HTTP_PROXY" ]] && echo "    HTTP_PROXY: $HTTP_PROXY"
    [[ -n "$HTTPS_PROXY" ]] && echo "    HTTPS_PROXY: $HTTPS_PROXY"
else
    echo "  Proxy: Not configured"
fi

# Export platform information for use by other scripts
export DETECTED_PLATFORM="$PLATFORM"
export DETECTED_ARCH="$ARCH"
export DETECTED_CPU_CORES="$CPU_CORES"
export DETECTED_TOTAL_RAM="$TOTAL_RAM"

echo ""
echo "✅ Platform detection complete!"
echo "Exported variables: DETECTED_PLATFORM, DETECTED_ARCH, DETECTED_CPU_CORES, DETECTED_TOTAL_RAM"