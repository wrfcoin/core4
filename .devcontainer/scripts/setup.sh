#!/bin/bash

# WRFCoin Development Container Setup Script
# This script runs when the devcontainer is created

set -e

echo "🚀 Setting up WRFCoin development environment..."

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[SETUP]${NC} $1"
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

# Ensure we're in the workspace directory
cd /workspace

print_status "Checking Node.js and npm versions..."
node --version
npm --version

print_status "Checking Rust installation..."
if command -v rustc &> /dev/null; then
    rustc --version
    cargo --version
    print_success "Rust is installed"
else
    print_warning "Rust not found, installing..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source ~/.cargo/env
    rustup component add rustfmt clippy
    print_success "Rust installed successfully"
fi

print_status "Checking Python installation..."
python3 --version
pip3 --version

# Install/update Python documentation tools
print_status "Installing Python documentation tools..."
pip3 install --user --upgrade \
    mkdocs \
    mkdocs-material \
    mkdocs-mermaid2-plugin \
    mkdocs-git-revision-date-localized-plugin \
    mkdocs-minify-plugin \
    pymdown-extensions

print_status "Setting up Git configuration..."
# Configure Git with safe directory (in case of ownership issues)
git config --global --add safe.directory /workspace
git config --global init.defaultBranch main

# Check if Git user is configured
if ! git config --global user.email > /dev/null; then
    print_warning "Git user not configured. Setting default values..."
    git config --global user.email "developer@wrfcoin.com"
    git config --global user.name "WRFCoin Developer"
fi

print_status "Setting up shell environment..."
# Add useful aliases to bashrc
if ! grep -q "# WRFCoin Development Aliases" ~/.bashrc; then
    cat >> ~/.bashrc << 'EOF'

# WRFCoin Development Aliases
alias ll='ls -alF'
alias la='ls -A'
alias l='ls -CF'
alias ..='cd ..'
alias ...='cd ../..'
alias ws='cd /workspace'
alias logs='docker-compose logs -f'
alias dc='docker-compose'
alias dcu='docker-compose up'
alias dcd='docker-compose down'
alias npmr='npm run'
alias npi='npm install'
alias nps='npm start'
alias npt='npm test'
alias npmb='npm run build'
alias cargob='cargo build'
alias cargor='cargo run'
alias cargot='cargo test'
alias cargoc='cargo clippy'
alias cargof='cargo fmt'

# WRFCoin project shortcuts
alias start-all='npm run start:all'
alias test-all='npm run test:all'
alias docs-serve='npm run docs:handbook:serve'
alias quality-check='npm run quality:check'

# Git shortcuts
alias gs='git status'
alias ga='git add'
alias gc='git commit'
alias gp='git push'
alias gl='git pull'
alias gd='git diff'
alias gb='git branch'
alias gco='git checkout'

EOF
    print_success "Development aliases added to ~/.bashrc"
fi

print_status "Setting up VS Code workspace settings..."
# Create workspace-specific VS Code settings if they don't exist
mkdir -p .vscode
if [ ! -f .vscode/settings.json ]; then
    cat > .vscode/settings.json << 'EOF'
{
  "typescript.preferences.includePackageJsonAutoImports": "on",
  "typescript.suggest.autoImports": true,
  "editor.formatOnSave": true,
  "editor.codeActionsOnSave": {
    "source.fixAll.eslint": "explicit",
    "source.organizeImports": "explicit"
  },
  "rust-analyzer.check.command": "clippy",
  "rust-analyzer.cargo.buildScripts.enable": true,
  "files.associations": {
    "*.rs": "rust",
    "*.toml": "toml",
    "*.sol": "solidity"
  },
  "search.exclude": {
    "**/node_modules": true,
    "**/target": true,
    "**/dist": true,
    "**/build": true,
    "**/.git": true,
    "**/coverage": true
  }
}
EOF
    print_success "VS Code workspace settings created"
fi

print_status "Setting up development environment variables..."
if [ ! -f .env.development ]; then
    cat > .env.development << 'EOF'
# WRFCoin Development Environment Variables
NODE_ENV=development
LOG_LEVEL=debug

# Database Configuration
DATABASE_URL=postgresql://postgres:password@test-db:5432/wrfcoin_test
REDIS_URL=redis://test-redis:6379

# API Configuration
API_PORT=4001
FRONTEND_PORT=3000
GRAPHQL_PORT=4001

# Blockchain Configuration
HARDHAT_NETWORK_URL=http://hardhat-node:8545
CHAIN_ID=31337

# IPFS Configuration
IPFS_API_URL=http://ipfs:5001

# Development Features
ENABLE_DEBUG_ROUTES=true
ENABLE_HOT_RELOAD=true
SKIP_AUTH_IN_DEVELOPMENT=false

# Testing Configuration
TEST_TIMEOUT=30000
PLAYWRIGHT_BROWSERS_PATH=/ms-playwright
EOF
    print_success "Development environment variables created"
fi

print_status "Verifying essential tools..."
essential_tools=("node" "npm" "git" "docker" "python3" "pip3")
missing_tools=()

for tool in "${essential_tools[@]}"; do
    if ! command -v "$tool" &> /dev/null; then
        missing_tools+=("$tool")
    fi
done

if [ ${#missing_tools[@]} -eq 0 ]; then
    print_success "All essential tools are available"
else
    print_error "Missing tools: ${missing_tools[*]}"
    exit 1
fi

# Check if Rust is available after potential installation
if command -v rustc &> /dev/null; then
    print_success "Rust toolchain verified"
else
    print_warning "Rust not available. You may need to restart the container."
fi

print_success "WRFCoin development environment setup completed!"
echo ""
echo -e "${BLUE}📚 Quick Start:${NC}"
echo "  • Run 'npm install' to install project dependencies"
echo "  • Run 'npm run start:all' to start all services"
echo "  • Run 'npm run docs:serve' to start documentation server"
echo "  • Run 'npm run test:all' to run all tests"
echo ""
echo -e "${BLUE}🔗 Useful URLs:${NC}"
echo "  • Frontend: http://localhost:3000"
echo "  • API Server: http://localhost:4001"
echo "  • Documentation: http://localhost:8001"
echo "  • GraphQL Playground: http://localhost:4001/graphql"
echo ""
echo -e "${GREEN}Happy coding! 🎉${NC}"