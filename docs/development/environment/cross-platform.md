# WRFCoin Cross-Platform Development Guide

This guide provides comprehensive instructions for setting up and troubleshooting the WRFCoin
development environment across Windows, macOS, and Linux platforms.

## 📋 Prerequisites

### All Platforms

- **Docker Desktop** (latest version)
- **Visual Studio Code** (latest version)
- **Dev Containers extension** (ms-vscode-remote.remote-containers)
- **Git** (for cloning the repository)
- **At least 8GB RAM** (16GB recommended)
- **At least 10GB free disk space**

### Platform-Specific Requirements

#### Windows

- **Windows 10/11** (Pro, Enterprise, or Education) with WSL 2 enabled
- **WSL 2** with Ubuntu 20.04+ distribution
- **Docker Desktop** with WSL 2 backend enabled
- **Windows Terminal** (recommended for better experience)

#### macOS

- **macOS 10.15 Catalina** or later
- **Docker Desktop for Mac**
- **Homebrew** (recommended for additional tools)

#### Linux

- **Ubuntu 20.04+**, **Debian 11+**, or equivalent
- **Docker Engine** and **Docker Compose**
- User must be in the `docker` group

## 🚀 Quick Setup Instructions

### 1. Install Prerequisites

#### Windows Setup

```powershell
# Install WSL 2 (run as Administrator)
wsl --install

# Install Ubuntu distribution
wsl --install -d Ubuntu-20.04

# Install Docker Desktop
# Download from: https://docs.docker.com/desktop/windows/install/
```

#### macOS Setup

```bash
# Install Docker Desktop
# Download from: https://docs.docker.com/desktop/mac/install/

# Install Homebrew (optional but recommended)
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install additional tools via Homebrew
brew install git curl wget
```

#### Linux Setup

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install -y docker.io docker-compose git curl wget

# Add user to docker group
sudo usermod -aG docker $USER
newgrp docker

# Start Docker service
sudo systemctl enable docker
sudo systemctl start docker
```

### 2. Clone and Open Project

```bash
# Clone the repository
git clone https://github.com/your-org/wrfcoin.git
cd wrfcoin

# Open in VS Code
code .
```

### 3. Open in Dev Container

1. VS Code will detect the `.devcontainer` configuration
2. Click "Reopen in Container" when prompted
3. Or use Command Palette: `Dev Containers: Reopen in Container`

## 🔧 Platform-Specific Configurations

### Windows-Specific Settings

#### Docker Desktop Configuration

```json
{
  "builder": {
    "gc": {
      "defaultKeepStorage": "20GB",
      "enabled": true
    }
  },
  "experimental": false,
  "features": {
    "buildkit": true
  },
  "wslEngineEnabled": true
}
```

#### VS Code Settings for Windows

Add to your VS Code `settings.json`:

```json
{
  "terminal.integrated.defaultProfile.windows": "WSL",
  "remote.WSL.fileWatcher.polling": true,
  "git.detectSubmodules": false
}
```

#### WSL 2 Memory Configuration

Create/edit `%USERPROFILE%\.wslconfig`:

```ini
[wsl2]
memory=8GB
processors=4
localhostForwarding=true
```

### macOS-Specific Settings

#### Docker Desktop Configuration

- **CPU**: 4-6 cores
- **Memory**: 8GB minimum, 12GB recommended
- **Swap**: 2GB
- **Disk**: 64GB minimum

#### File Sharing Performance

Add to `.zshrc` or `.bash_profile`:

```bash
export DOCKER_BUILDKIT=1
export COMPOSE_DOCKER_CLI_BUILD=1
```

### Linux-Specific Settings

#### Docker Daemon Configuration

Create/edit `/etc/docker/daemon.json`:

```json
{
  "features": {
    "buildkit": true
  },
  "default-address-pools": [
    {
      "base": "172.17.0.0/12",
      "size": 20
    },
    {
      "base": "192.168.0.0/16",
      "size": 24
    }
  ]
}
```

#### System Resource Limits

Add to `/etc/security/limits.conf`:

```
* soft nofile 65536
* hard nofile 65536
* soft nproc 32768
* hard nproc 32768
```

## 🐛 Common Issues and Solutions

### Windows Issues

#### Issue: WSL 2 Installation Fails

**Solution:**

```powershell
# Enable Windows features
dism.exe /online /enable-feature /featurename:Microsoft-Windows-Subsystem-Linux /all /norestart
dism.exe /online /enable-feature /featurename:VirtualMachinePlatform /all /norestart

# Restart and set WSL 2 as default
wsl --set-default-version 2
```

#### Issue: Docker Desktop Won't Start

**Solutions:**

1. Check Windows features:

   - Hyper-V enabled
   - Windows Subsystem for Linux enabled
   - Virtual Machine Platform enabled

2. Reset Docker Desktop:
   ```powershell
   # Completely reset Docker Desktop
   docker system prune -a
   # Restart Docker Desktop
   ```

#### Issue: Port Binding Failures

**Solution:**

```powershell
# Check if ports are in use
netstat -ano | findstr :3000
netstat -ano | findstr :4001

# Kill process using port (replace PID)
taskkill /PID <PID> /F
```

#### Issue: File Watching Problems

**Solution:** Add to VS Code settings:

```json
{
  "files.watcherExclude": {
    "**/node_modules/**": true,
    "**/target/**": true
  }
}
```

### macOS Issues

#### Issue: Docker Desktop Performance

**Solutions:**

1. Enable VirtioFS:

   - Docker Desktop → Settings → Experimental Features
   - Enable "Use new Virtualization framework"
   - Enable "VirtioFS accelerated directory sharing"

2. Adjust resource allocation:
   - Increase CPU to 6 cores
   - Increase Memory to 12GB
   - Use disk space efficiently

#### Issue: Permission Denied Errors

**Solution:**

```bash
# Fix permissions in container
sudo chown -R vscode:vscode /workspace

# Or in VS Code Dev Container
sudo usermod -a -G docker vscode
```

#### Issue: Network Connectivity Problems

**Solution:**

```bash
# Reset Docker network
docker network prune -f
docker system prune -f

# Restart Docker Desktop
```

### Linux Issues

#### Issue: Permission Denied for Docker

**Solution:**

```bash
# Add user to docker group
sudo usermod -aG docker $USER
newgrp docker

# Fix docker socket permissions
sudo chmod 666 /var/run/docker.sock
```

#### Issue: Out of Disk Space

**Solution:**

```bash
# Clean Docker system
docker system prune -a --volumes

# Check disk usage
df -h
docker system df

# Remove unused images
docker image prune -a
```

#### Issue: Container Build Failures

**Solution:**

```bash
# Increase file descriptors
echo "fs.inotify.max_user_watches=524288" | sudo tee -a /etc/sysctl.conf
sudo sysctl -p

# Clear build cache
docker builder prune -a
```

## 🔍 Testing Your Setup

### Quick Health Check Script

Create `test-devcontainer.sh`:

```bash
#!/bin/bash

echo "🔍 Testing WRFCoin Development Container Setup"
echo "============================================="

# Check Docker
echo "📦 Checking Docker..."
if docker --version > /dev/null 2>&1; then
    echo "✅ Docker is installed: $(docker --version)"
else
    echo "❌ Docker is not installed or not accessible"
    exit 1
fi

# Check Docker Compose
echo "📦 Checking Docker Compose..."
if docker-compose --version > /dev/null 2>&1; then
    echo "✅ Docker Compose is available: $(docker-compose --version)"
else
    echo "❌ Docker Compose is not available"
fi

# Check VS Code
echo "📝 Checking VS Code..."
if code --version > /dev/null 2>&1; then
    echo "✅ VS Code is installed: $(code --version | head -1)"
else
    echo "❌ VS Code is not in PATH"
fi

# Test container build
echo "🔨 Testing container build..."
if docker build -t wrfcoin-test .devcontainer/ > /dev/null 2>&1; then
    echo "✅ Container builds successfully"
    docker rmi wrfcoin-test > /dev/null 2>&1
else
    echo "❌ Container build failed"
fi

# Check available resources
echo "💾 Checking system resources..."
echo "Available memory: $(free -h | awk '/^Mem:/ { print $7 }' 2>/dev/null || echo 'N/A')"
echo "Available disk: $(df -h . | awk 'NR==2 { print $4 }')"

echo ""
echo "🎉 Health check complete!"
```

### Performance Benchmark Script

Create `benchmark-devcontainer.sh`:

```bash
#!/bin/bash

echo "⏱️  Running WRFCoin Dev Container Performance Tests"
echo "=================================================="

# Time container startup
echo "📊 Testing container startup time..."
start_time=$(date +%s)
docker-compose -f .devcontainer/docker-compose.dev.yml up -d workspace
end_time=$(date +%s)
startup_time=$((end_time - start_time))
echo "Container startup time: ${startup_time}s"

# Test build performance
echo "📊 Testing build performance..."
build_start=$(date +%s)
docker exec wrfcoin-devcontainer bash -c "cd /workspace && npm run build" > /dev/null 2>&1
build_end=$(date +%s)
build_time=$((build_end - build_start))
echo "Build time: ${build_time}s"

# Test Rust compilation
echo "📊 Testing Rust compilation..."
rust_start=$(date +%s)
docker exec wrfcoin-devcontainer bash -c "cd /workspace/blockchain-core && cargo build" > /dev/null 2>&1
rust_end=$(date +%s)
rust_time=$((rust_end - rust_start))
echo "Rust compilation time: ${rust_time}s"

# Cleanup
docker-compose -f .devcontainer/docker-compose.dev.yml down

echo ""
echo "📈 Performance Summary:"
echo "  Container startup: ${startup_time}s"
echo "  Build time: ${build_time}s"
echo "  Rust compilation: ${rust_time}s"
```

## 📚 Platform-Specific Tips

### Windows Development Tips

1. **Use WSL 2 for better performance**

   - Clone repository inside WSL filesystem (`/home/user/`)
   - Use `\\wsl$\Ubuntu\home\user\wrfcoin` for Windows access

2. **Optimize file watching**

   - Exclude large directories from file watching
   - Use `.gitignore` patterns in VS Code settings

3. **PowerShell alternatives**
   - Use Windows Terminal with PowerShell Core
   - Consider Git Bash for Unix-like experience

### macOS Development Tips

1. **Resource optimization**

   - Monitor Activity Monitor for Docker resource usage
   - Use `docker stats` to monitor container resources

2. **File system performance**

   - Enable VirtioFS for better file sharing performance
   - Consider using volumes for `node_modules`

3. **Network troubleshooting**
   - Use `docker inspect` to check network configuration
   - Monitor port conflicts with `lsof -i :PORT`

### Linux Development Tips

1. **System optimization**

   - Increase file descriptor limits
   - Monitor disk space with `docker system df`

2. **Security considerations**

   - Run containers as non-root user
   - Use docker rootless mode when possible

3. **Performance monitoring**
   - Use `htop` to monitor system resources
   - Monitor Docker daemon logs: `journalctl -u docker`

## 🆘 Getting Help

### Debug Information Collection

Run this script when reporting issues:

```bash
#!/bin/bash
echo "🔍 WRFCoin Dev Container Debug Information"
echo "========================================"

echo "System Information:"
uname -a
echo ""

echo "Docker Information:"
docker version
docker info | grep -E "(CPUs|Total Memory|Operating System)"
echo ""

echo "Container Status:"
docker ps -a
echo ""

echo "Volume Status:"
docker volume ls
echo ""

echo "Network Status:"
docker network ls
echo ""

echo "Recent Docker Logs:"
docker logs wrfcoin-devcontainer --tail 50 2>/dev/null || echo "Container not running"
```

### Support Channels

1. **GitHub Issues**: Report bugs and feature requests
2. **Slack/Discord**: Real-time community support
3. **Documentation**: Check latest docs for updates
4. **Stack Overflow**: Use tags `wrfcoin`, `devcontainer`, `docker`

### Useful Commands

```bash
# Reset entire development environment
docker-compose -f .devcontainer/docker-compose.dev.yml down --volumes
docker system prune -a

# Rebuild containers from scratch
docker-compose -f .devcontainer/docker-compose.dev.yml build --no-cache

# Monitor resource usage
docker stats

# Check container logs
docker logs wrfcoin-devcontainer -f

# Execute commands in container
docker exec -it wrfcoin-devcontainer bash
```

## 🎯 Next Steps

After successfully setting up your development environment:

1. **Run the test suite**: `npm run test:all`
2. **Start development servers**: `npm run start:all`
3. **Build the project**: `npm run build`
4. **Check code quality**: `npm run quality:check`
5. **Read the project documentation**: `npm run docs:serve`

Happy coding! 🚀
