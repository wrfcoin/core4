# WRFCoin Dev Container Troubleshooting Guide

This comprehensive guide helps you diagnose and resolve common issues with the WRFCoin development
container across all supported platforms.

## =' Quick Diagnostics

### Health Check Script

Run this command first to identify issues:

```bash
./.devcontainer/scripts/detect-platform.sh
./.devcontainer/scripts/validate-cross-platform.sh
```

### Emergency Reset

If you need to start fresh:

```bash
# Stop all containers
docker-compose -f .devcontainer/docker-compose.dev.yml down --volumes

# Clean Docker system
docker system prune -a --volumes

# Rebuild from scratch
docker-compose -f .devcontainer/docker-compose.dev.yml build --no-cache
```

## = Common Issues by Category

### =� Container Startup Issues

#### Issue: Container fails to build

**Error Messages:**

- `failed to solve: process "/bin/sh -c ..." did not complete successfully`
- `ERROR: failed to build: failed to solve`

**Solutions:**

1. **Check Docker Desktop status:**

   ```bash
   docker info
   # Should show "Server" section without errors
   ```

2. **Clear build cache:**

   ```bash
   docker builder prune -a
   ```

3. **Rebuild with no cache:**

   ```bash
   docker build --no-cache -t wrfcoin-dev .devcontainer/
   ```

4. **Check available disk space:**
   ```bash
   df -h
   docker system df
   ```

#### Issue: Container starts but services fail

**Error Messages:**

- `Service 'workspace' failed to build`
- `Container exited with code 1`

**Solutions:**

1. **Check individual service logs:**

   ```bash
   docker-compose -f .devcontainer/docker-compose.dev.yml logs workspace
   docker-compose -f .devcontainer/docker-compose.dev.yml logs test-db
   ```

2. **Verify port availability:**

   ```bash
   # Windows
   netstat -ano | findstr :5432
   netstat -ano | findstr :6379

   # macOS/Linux
   lsof -i :5432
   lsof -i :6379
   ```

3. **Check Docker network:**
   ```bash
   docker network ls
   docker network inspect devcontainer_default
   ```

### = Connectivity Issues

#### Issue: Cannot connect to databases

**Error Messages:**

- `ECONNREFUSED` when connecting to PostgreSQL/Redis
- `dial tcp: lookup test-db: no such host`

**Solutions:**

1. **Wait for services to initialize:**

   ```bash
   # Wait 60 seconds after container startup
   sleep 60

   # Test connectivity
   docker exec wrfcoin-devcontainer-workspace-1 nc -z test-db 5432
   docker exec wrfcoin-devcontainer-workspace-1 nc -z test-redis 6379
   ```

2. **Check service health:**

   ```bash
   docker-compose -f .devcontainer/docker-compose.dev.yml ps
   ```

3. **Restart specific services:**
   ```bash
   docker-compose -f .devcontainer/docker-compose.dev.yml restart test-db
   docker-compose -f .devcontainer/docker-compose.dev.yml restart test-redis
   ```

#### Issue: Port forwarding not working

**Error Messages:**

- `This site can't be reached` when accessing localhost:3000
- `Connection refused` to development servers

**Solutions:**

1. **Check VS Code port forwarding:**

   - Open VS Code Command Palette
   - Run "Ports: Focus on Ports View"
   - Verify ports 3000, 4001, 5432, 6379 are forwarded

2. **Manual port forwarding:**

   ```bash
   # Add these to .devcontainer/devcontainer.json
   "forwardPorts": [3000, 4001, 5432, 6379, 8080]
   ```

3. **Test from within container:**
   ```bash
   docker exec -it wrfcoin-devcontainer-workspace-1 bash
   curl localhost:3000
   ```

### =� File System Issues

#### Issue: File changes not detected (hot reload broken)

**Error Messages:**

- Development server doesn't restart on file changes
- `ENOSPC: System limit for number of file watchers reached`

**Solutions:**

**Linux:**

```bash
# Increase file watcher limit
echo fs.inotify.max_user_watches=524288 | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

**macOS:**

```bash
# Enable VirtioFS in Docker Desktop
# Docker Desktop � Settings � Experimental Features
# Enable "Use new Virtualization framework"
# Enable "VirtioFS accelerated directory sharing"
```

**Windows WSL2:**

```bash
# Use WSL2 filesystem, not Windows filesystem
# Move project to /home/user/ instead of /mnt/c/
```

**VS Code settings (add to settings.json):**

```json
{
  "files.watcherExclude": {
    "**/node_modules/**": true,
    "**/target/**": true,
    "**/.git/**": true
  }
}
```

#### Issue: Permission denied errors

**Error Messages:**

- `EACCES: permission denied`
- `npm ERR! Error: EACCES`

**Solutions:**

1. **Fix container user permissions:**

   ```bash
   docker exec -u root wrfcoin-devcontainer-workspace-1 chown -R vscode:vscode /workspace
   ```

2. **Linux-specific fix:**

   ```bash
   # Ensure user is in docker group
   sudo usermod -aG docker $USER
   newgrp docker
   ```

3. **Windows-specific fix:**
   ```bash
   # Run VS Code as administrator if needed
   # Or fix WSL file permissions
   wsl --shutdown
   wsl
   ```

### =' Build & Compilation Issues

#### Issue: NPM install fails

**Error Messages:**

- `npm ERR! network request failed`
- `npm ERR! code ENOTFOUND`

**Solutions:**

1. **Check network connectivity:**

   ```bash
   docker exec wrfcoin-devcontainer-workspace-1 ping google.com
   ```

2. **Clear npm cache:**

   ```bash
   docker exec wrfcoin-devcontainer-workspace-1 npm cache clean --force
   ```

3. **Use alternative registry:**

   ```bash
   docker exec wrfcoin-devcontainer-workspace-1 npm config set registry https://registry.npmjs.org/
   ```

4. **Corporate network fix:**
   ```bash
   # Add to .devcontainer/Dockerfile.dev before npm install
   RUN npm config set strict-ssl false
   RUN npm config set proxy http://your-proxy:port
   RUN npm config set https-proxy http://your-proxy:port
   ```

#### Issue: Rust compilation fails

**Error Messages:**

- `cargo: command not found`
- `linker 'cc' not found`

**Solutions:**

1. **Verify Rust installation:**

   ```bash
   docker exec wrfcoin-devcontainer-workspace-1 rustc --version
   docker exec wrfcoin-devcontainer-workspace-1 cargo --version
   ```

2. **Reinstall Rust in container:**

   ```bash
   docker exec wrfcoin-devcontainer-workspace-1 bash -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
   ```

3. **Update PATH:**
   ```bash
   docker exec wrfcoin-devcontainer-workspace-1 bash -c "source ~/.cargo/env && cargo check"
   ```

#### Issue: TypeScript compilation errors

**Error Messages:**

- `error TS2307: Cannot find module`
- `error TS2345: Argument of type 'X' is not assignable`

**Solutions:**

1. **Update TypeScript and types:**

   ```bash
   docker exec wrfcoin-devcontainer-workspace-1 npm update typescript @types/node
   ```

2. **Clear TypeScript cache:**

   ```bash
   docker exec wrfcoin-devcontainer-workspace-1 npx tsc --build --clean
   ```

3. **Verify tsconfig.json:**
   ```bash
   docker exec wrfcoin-devcontainer-workspace-1 npx tsc --showConfig
   ```

## =� Platform-Specific Issues

### Windows (WSL2) Issues

#### Issue: WSL2 not starting

**Error Messages:**

- `WslRegisterDistribution failed with error: 0x80370102`
- `The Windows Subsystem for Linux has not been enabled`

**Solutions:**

1. **Enable Windows features:**

   ```powershell
   # Run as Administrator
   dism.exe /online /enable-feature /featurename:Microsoft-Windows-Subsystem-Linux /all /norestart
   dism.exe /online /enable-feature /featurename:VirtualMachinePlatform /all /norestart
   ```

2. **Set WSL2 as default:**

   ```powershell
   wsl --set-default-version 2
   ```

3. **Update WSL kernel:**
   ```powershell
   # Download from: https://aka.ms/wsl2kernel
   ```

#### Issue: Docker Desktop won't start on Windows

**Error Messages:**

- `Docker Desktop - Unexpected WSL error`
- `Hardware assisted virtualization and data execution protection must be enabled`

**Solutions:**

1. **Enable Hyper-V:**

   ```powershell
   # Run as Administrator
   Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V -All
   ```

2. **Check BIOS settings:**

   - Enable Intel VT-x or AMD-V
   - Enable Intel VT-d or AMD IOMMU
   - Disable Secure Boot if necessary

3. **Reset Docker Desktop:**
   ```powershell
   # Uninstall and reinstall Docker Desktop
   # Or reset to factory defaults in Docker Desktop settings
   ```

#### Issue: Poor performance on Windows

**Symptoms:**

- Slow file operations
- Long build times
- High CPU usage

**Solutions:**

1. **Use WSL2 filesystem:**

   ```bash
   # Clone project in WSL2, not Windows filesystem
   cd /home/username
   git clone https://github.com/your-org/wrfcoin.git
   ```

2. **Configure WSL2 memory:** Create `%USERPROFILE%\.wslconfig`:

   ```ini
   [wsl2]
   memory=8GB
   processors=4
   localhostForwarding=true
   ```

3. **Exclude from Windows Defender:**
   - Add WSL2 folders to Windows Defender exclusions
   - Add Docker data folder to exclusions

### macOS Issues

#### Issue: Docker Desktop performance issues

**Symptoms:**

- Slow container startup
- High CPU usage
- Poor file I/O performance

**Solutions:**

1. **Enable VirtioFS:**

   ```bash
   # Docker Desktop � Settings � Experimental Features
   # Enable "Use new Virtualization framework"
   # Enable "VirtioFS accelerated directory sharing"
   ```

2. **Increase resource allocation:**

   ```bash
   # Docker Desktop � Settings � Resources
   # CPU: 6 cores
   # Memory: 12GB
   # Swap: 2GB
   ```

3. **Use Docker volumes for node_modules:**
   ```yaml
   # Add to docker-compose.dev.yml
   volumes:
     - ../..:/workspace:cached
     - node_modules:/workspace/node_modules
   ```

#### Issue: Apple Silicon compatibility

**Error Messages:**

- `The requested image's platform (linux/amd64) does not match`
- `qemu: uncaught target signal 11`

**Solutions:**

1. **Use multi-platform images:**

   ```dockerfile
   # In Dockerfile.dev
   FROM --platform=linux/amd64 node:20-bullseye
   # Or
   FROM node:20-bullseye  # Let Docker choose appropriate platform
   ```

2. **Install Rosetta 2:**

   ```bash
   /usr/sbin/softwareupdate --install-rosetta --agree-to-license
   ```

3. **Use ARM64 native images when available:**
   ```yaml
   # In docker-compose.dev.yml
   platform: linux/arm64/v8 # For Apple Silicon
   ```

### Linux Issues

#### Issue: User not in docker group

**Error Messages:**

- `permission denied while trying to connect to the Docker daemon socket`
- `dial unix /var/run/docker.sock: connect: permission denied`

**Solutions:**

1. **Add user to docker group:**

   ```bash
   sudo usermod -aG docker $USER
   newgrp docker
   ```

2. **Fix socket permissions:**

   ```bash
   sudo chmod 666 /var/run/docker.sock
   ```

3. **Restart Docker service:**
   ```bash
   sudo systemctl restart docker
   ```

#### Issue: SELinux/AppArmor conflicts

**Error Messages:**

- `Permission denied` despite correct file permissions
- `audit: type=1400` in system logs

**Solutions:**

**SELinux:**

```bash
# Check SELinux status
getenforce

# Set permissive mode temporarily
sudo setenforce 0

# Or create custom policy
sudo setsebool -P container_manage_cgroup true
```

**AppArmor:**

```bash
# Check AppArmor status
sudo aa-status

# Disable specific profile if needed
sudo aa-disable /usr/bin/docker
```

#### Issue: Insufficient file descriptors

**Error Messages:**

- `too many open files`
- `EMFILE: too many open files`

**Solutions:**

1. **Increase limits temporarily:**

   ```bash
   ulimit -n 65536
   ```

2. **Increase limits permanently:**
   ```bash
   # Add to /etc/security/limits.conf
   * soft nofile 65536
   * hard nofile 65536
   ```

## =

Debugging Tools and Commands

### Container Inspection

```bash
# List all containers
docker ps -a

# Inspect container configuration
docker inspect wrfcoin-devcontainer-workspace-1

# Check container logs
docker logs wrfcoin-devcontainer-workspace-1 --tail 50 -f

# Execute commands in container
docker exec -it wrfcoin-devcontainer-workspace-1 bash
```

### Network Debugging

```bash
# List Docker networks
docker network ls

# Inspect network configuration
docker network inspect devcontainer_default

# Test connectivity between containers
docker exec wrfcoin-devcontainer-workspace-1 ping test-db
docker exec wrfcoin-devcontainer-workspace-1 nc -zv test-db 5432
```

### Volume and File System Debugging

```bash
# List Docker volumes
docker volume ls

# Inspect volume
docker volume inspect devcontainer_postgres-data

# Check file permissions
docker exec wrfcoin-devcontainer-workspace-1 ls -la /workspace

# Monitor file changes
docker exec wrfcoin-devcontainer-workspace-1 inotifywait -m /workspace
```

### Performance Monitoring

```bash
# Monitor container resource usage
docker stats

# Monitor system resources
htop  # Linux
top   # macOS
taskmgr  # Windows

# Check disk usage
docker system df
df -h
```

### VS Code Dev Container Debugging

```bash
# Check Dev Container logs
# Command Palette � "Dev Containers: Show Container Log"

# Rebuild container
# Command Palette � "Dev Containers: Rebuild Container"

# Open in new window
# Command Palette � "Dev Containers: Reopen in Container"
```

## =� Getting Help

### Information to Collect

When reporting issues, include:

1. **Platform information:**

   ```bash
   ./.devcontainer/scripts/detect-platform.sh > debug-info.txt
   ```

2. **Docker information:**

   ```bash
   docker version >> debug-info.txt
   docker info >> debug-info.txt
   ```

3. **Container logs:**

   ```bash
   docker-compose -f .devcontainer/docker-compose.dev.yml logs >> debug-info.txt
   ```

4. **VS Code information:**
   ```bash
   code --version >> debug-info.txt
   ```

### Support Channels

1. **GitHub Issues**: Report bugs with debug information
2. **Team Chat**: Quick questions and real-time help
3. **Documentation**: Check latest updates and known issues
4. **Stack Overflow**: Community support with `wrfcoin`, `devcontainer` tags

### Emergency Contacts

- **DevOps Team**: Container infrastructure issues
- **Platform Team**: Development environment problems
- **Security Team**: Permission and access issues

## =� Performance Optimization

### Docker Configuration

```json
// daemon.json
{
  "features": {
    "buildkit": true
  },
  "default-address-pools": [
    {
      "base": "172.17.0.0/12",
      "size": 20
    }
  ]
}
```

### Resource Allocation Guidelines

| Platform            | CPU       | Memory  | Disk  |
| ------------------- | --------- | ------- | ----- |
| Windows WSL2        | 4-6 cores | 8-12GB  | 50GB+ |
| macOS Intel         | 4-6 cores | 8-16GB  | 50GB+ |
| macOS Apple Silicon | 6-8 cores | 12-16GB | 50GB+ |
| Linux               | 4-8 cores | 8-16GB  | 50GB+ |

### Build Optimization

```bash
# Use BuildKit
export DOCKER_BUILDKIT=1

# Use build cache
docker build --cache-from wrfcoin-dev .

# Multi-stage builds for smaller images
# Use .dockerignore to exclude unnecessary files
```

---

## =� Troubleshooting Checklist

Before reporting issues, try these steps:

- [ ] Run platform detection script
- [ ] Run cross-platform validation script
- [ ] Check Docker Desktop is running
- [ ] Verify available disk space (>10GB)
- [ ] Restart Docker Desktop/service
- [ ] Clear Docker cache (`docker system prune`)
- [ ] Rebuild container without cache
- [ ] Check VS Code Dev Container logs
- [ ] Test with fresh git clone
- [ ] Verify network connectivity

If issues persist after trying these steps, collect debug information and contact support.

---

**Last Updated**: July 2025  
**Version**: 1.0.0  
**Maintained by**: WRFCoin Platform Team
