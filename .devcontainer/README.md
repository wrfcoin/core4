# WRFCoin Development Container

This directory contains the VS Code DevContainer configuration for the WRFCoin platform, providing a
complete, reproducible development environment with all necessary tools and dependencies
pre-installed.

## 🚀 Quick Start

1. **Prerequisites**:

   - [VS Code](https://code.visualstudio.com/)
   - [Dev Containers extension](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers)
   - [Docker Desktop](https://www.docker.com/products/docker-desktop/)

2. **Open in Container**:

   - Open the project in VS Code
   - Press `Ctrl+Shift+P` (or `Cmd+Shift+P` on Mac)
   - Type "Dev Containers: Reopen in Container"
   - Select the command and wait for the container to build

3. **Start Development**:

   ```bash
   # Install dependencies (if not already done)
   npm install

   # Start all services
   npm run start:all

   # Run tests
   npm run test:all
   ```

## 📦 What's Included

### Development Tools

- **Node.js 20** with npm and pnpm
- **Rust** (latest stable) with cargo, clippy, and rustfmt
- **Python 3** with pip and documentation tools
- **Docker-in-Docker** for container operations
- **Git** with helpful aliases and pre-commit hooks

### VS Code Extensions (50+)

- **Rust Development**: rust-analyzer, LLDB debugger, Crates.io support
- **TypeScript/JavaScript**: Enhanced TypeScript, Prettier, ESLint
- **React Development**: React snippets, auto-rename tags, path intellisense
- **Solidity**: Solidity language support, visual auditor, Truffle
- **Testing**: Playwright, Jest, Test Explorer
- **Documentation**: Markdown support, Mermaid diagrams, YAML
- **DevOps**: Docker, Kubernetes, Terraform support
- **Productivity**: GitLens, GitHub Copilot, Thunder Client

### Services

- **PostgreSQL 15** (test database)
- **Redis 7** (caching and sessions)
- **IPFS** (distributed storage)
- **Hardhat Node** (local blockchain)

### Documentation Tools

- **MkDocs** with Material theme
- **Mermaid** diagrams
- **Markdown** processing
- **API documentation** generation

## 🔧 Configuration Files

- `devcontainer.json` - Main DevContainer configuration
- `docker-compose.dev.yml` - Multi-service development environment
- `Dockerfile.dev` - Main development container
- `Dockerfile.hardhat` - Hardhat blockchain node
- `scripts/setup.sh` - Initial container setup
- `scripts/post-create.sh` - Post-creation configuration
- `scripts/post-start.sh` - Startup tasks and health checks

## 🌐 Available Services

When the container starts, these services are available:

| Service       | URL                           | Purpose                  |
| ------------- | ----------------------------- | ------------------------ |
| Frontend      | http://localhost:3000         | React development server |
| API Server    | http://localhost:4001         | Main application API     |
| GraphQL       | http://localhost:4001/graphql | GraphQL playground       |
| Documentation | http://localhost:8001         | MkDocs documentation     |
| PostgreSQL    | localhost:5432                | Development database     |
| Redis         | localhost:6379                | Cache and sessions       |
| IPFS API      | localhost:5001                | IPFS node API            |
| IPFS Gateway  | localhost:8080                | IPFS gateway             |
| Hardhat Node  | localhost:8545                | Local blockchain RPC     |

## 🛠️ Development Workflow

### Starting Services

```bash
# Start all services
wrf-start
# or
npm run start:all

# Start individual services
wrf-frontend    # Frontend only
wrf-api        # Backend API only
wrf-graphql    # GraphQL server only
```

### Running Tests

```bash
# All tests
wrf-test
# or
npm run test:all

# Specific test suites
npm run test:unit              # Unit tests
npm run test:e2e               # End-to-end tests
npm run test:smart-contracts   # Smart contract tests
npm run test:rust              # Rust tests
```

### Code Quality

```bash
# Check code quality
wrf-quality
# or
npm run quality:check

# Fix formatting and linting
npm run quality:fix
```

### Documentation

```bash
# Start documentation server
wrf-docs
# or
npm run docs:handbook:serve

# Build all documentation
npm run docs:build

# Validate documentation style
npm run docs:style-check
```

## 🔍 Development Shortcuts

The container includes helpful aliases:

### WRFCoin Commands

- `wrf-start` - Start all services
- `wrf-test` - Run all tests
- `wrf-build` - Build all projects
- `wrf-docs` - Start documentation server
- `wrf-quality` - Check code quality
- `wrf-clean` - Clean up containers and cache

### Standard Commands

- `ws` - Navigate to workspace
- `dc` - Docker Compose shorthand
- `npmr` - npm run shorthand
- `gs` - git status
- `ga` - git add
- `gc` - git commit

## 🐛 Troubleshooting

### Container Won't Start

1. **Check Docker**: Ensure Docker Desktop is running
2. **Free Ports**: Make sure ports 3000, 4001, 5432, 6379 aren't in use
3. **Rebuild Container**: Use "Dev Containers: Rebuild Container"

### Services Not Available

```bash
# Check service status
wrf-logs

# Restart services
wrf-restart

# Reset everything
wrf-reset
```

### Database Connection Issues

```bash
# Check database status
nc -z test-db 5432

# Connect to database manually
PGPASSWORD=password psql -h test-db -U postgres -d wrfcoin_test

# Reset database
docker-compose restart test-db
```

### Rust Compilation Issues

```bash
# Update Rust toolchain
rustup update

# Clear Rust cache
cargo clean

# Rebuild with verbose output
cargo build --verbose
```

## 📁 Volume Mounts

The container uses several volume mounts for performance and persistence:

- `node_modules` - Node.js dependencies cache
- `cargo_cache` - Rust dependencies cache
- `cargo_target` - Rust build artifacts
- `vscode_extensions` - VS Code extensions
- `postgres_data` - Database data
- `redis_data` - Redis data
- `ipfs_data` - IPFS data

## 🔒 Security

- Container runs as non-root `vscode` user
- Docker socket is mounted read-only
- Sensitive operations require explicit privileges
- Development database uses non-production credentials

## 🚀 Performance Optimization

- **Volume Caches**: Dependencies and build artifacts are cached in Docker volumes
- **Multi-stage Build**: Optimized Docker images for faster startup
- **Parallel Installation**: Dependencies install in parallel where possible
- **Resource Limits**: Configured for optimal performance on typical development machines

## 🎯 Next Steps

After the container is running:

1. **Explore the Codebase**: Use VS Code's integrated terminal and file explorer
2. **Run Tests**: Ensure everything works with `npm run test:all`
3. **Start Coding**: The environment is fully configured and ready
4. **Read Documentation**: Visit http://localhost:8001 when docs are served
5. **Join Development**: Follow the contributor guidelines in the main README

## 📚 Additional Resources

- [VS Code Dev Containers Documentation](https://code.visualstudio.com/docs/remote/containers)
- [Docker Compose Documentation](https://docs.docker.com/compose/)
- [WRFCoin Platform Documentation](../docs/)
- [Contributing Guidelines](../CONTRIBUTING.md)

---

**Happy coding with WRFCoin! 🌟**
