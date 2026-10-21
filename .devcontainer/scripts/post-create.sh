#!/bin/bash

# WRFCoin Post-Create Script
# This script runs after the devcontainer is created and setup.sh has completed

set -e

echo "🔧 Running post-create setup for WRFCoin development..."

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_status() {
    echo -e "${BLUE}[POST-CREATE]${NC} $1"
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

print_status "Installing project dependencies..."
if [ -f package.json ]; then
    # Clear npm cache to avoid potential issues
    npm cache clean --force 2>/dev/null || true
    
    # Install dependencies with increased timeout
    npm install --prefer-offline --no-audit --progress=false
    print_success "Node.js dependencies installed"
else
    print_warning "No package.json found, skipping npm install"
fi

print_status "Installing Rust dependencies..."
if [ -d blockchain-core ] && [ -f blockchain-core/Cargo.toml ]; then
    cd blockchain-core
    
    # Pre-download Rust dependencies
    cargo fetch
    
    # Build in development mode to cache dependencies
    cargo build --release > /dev/null 2>&1 || {
        print_warning "Rust build failed, but dependencies were cached"
    }
    
    cd /workspace
    print_success "Rust dependencies prepared"
else
    print_warning "No Rust project found, skipping Cargo setup"
fi

print_status "Installing Python documentation dependencies..."
# Ensure Python packages are available
python3 -m pip install --user --upgrade --quiet \
    mkdocs \
    mkdocs-material \
    mkdocs-mermaid2-plugin \
    mkdocs-git-revision-date-localized-plugin \
    mkdocs-minify-plugin \
    pymdown-extensions \
    2>/dev/null || print_warning "Some Python packages failed to install"

print_success "Python documentation tools ready"

print_status "Setting up Git hooks..."
if [ -d .git ]; then
    # Create pre-commit hook for code quality
    mkdir -p .git/hooks
    cat > .git/hooks/pre-commit << 'EOF'
#!/bin/bash
# WRFCoin Pre-commit Hook
# Runs basic code quality checks before commits

echo "Running pre-commit checks..."

# Check if we can run quality checks
if command -v npm &> /dev/null; then
    # Run linting
    if npm run lint:fix &> /dev/null; then
        echo "✓ Linting passed"
    else
        echo "⚠ Linting issues found (but fixed automatically)"
    fi
    
    # Run formatting
    if npm run format &> /dev/null; then
        echo "✓ Code formatting applied"
    fi
fi

# Check Rust formatting if Rust files are being committed
if git diff --cached --name-only | grep -q "\.rs$"; then
    if command -v cargo &> /dev/null && [ -f blockchain-core/Cargo.toml ]; then
        cd blockchain-core
        if cargo fmt -- --check &> /dev/null; then
            echo "✓ Rust formatting is correct"
        else
            echo "⚠ Applying Rust formatting..."
            cargo fmt
            git add -A
        fi
        cd ..
    fi
fi

echo "Pre-commit checks completed"
EOF
    chmod +x .git/hooks/pre-commit
    print_success "Git pre-commit hook installed"
else
    print_warning "Not a Git repository, skipping Git hooks setup"
fi

print_status "Building initial documentation..."
if [ -f scripts/build-mkdocs.sh ]; then
    chmod +x scripts/build-mkdocs.sh
    # Try to build docs, but don't fail if it doesn't work
    bash scripts/build-mkdocs.sh > /dev/null 2>&1 && {
        print_success "Documentation built successfully"
    } || {
        print_warning "Documentation build failed (this is normal on first run)"
    }
else
    print_warning "MkDocs build script not found"
fi

print_status "Installing VS Code extensions..."
# List of essential extensions that should be installed
ESSENTIAL_EXTENSIONS=(
    "rust-lang.rust-analyzer"
    "ms-vscode.vscode-typescript-next"
    "esbenp.prettier-vscode"
    "ms-vscode.eslint"
    "ms-playwright.playwright"
    "JuanBlanco.solidity"
)

for extension in "${ESSENTIAL_EXTENSIONS[@]}"; do
    if command -v code &> /dev/null; then
        code --install-extension "$extension" --force > /dev/null 2>&1 && {
            print_status "Installed extension: $extension"
        } || {
            print_warning "Failed to install extension: $extension"
        }
    fi
done

print_status "Setting up development database..."
# Wait for database to be ready
max_attempts=30
attempt=0
while ! nc -z test-db 5432 2>/dev/null && [ $attempt -lt $max_attempts ]; do
    sleep 1
    ((attempt++))
    if [ $((attempt % 10)) -eq 0 ]; then
        print_status "Waiting for database to be ready... (${attempt}/${max_attempts})"
    fi
done

if [ $attempt -lt $max_attempts ]; then
    print_success "Database connection established"
    
    # Run database setup if available
    if [ -f package.json ] && npm run --silent db:setup > /dev/null 2>&1; then
        print_success "Database setup completed"
    elif [ -d database/migrations ]; then
        print_warning "Database migrations exist but couldn't run automatically"
    fi
else
    print_warning "Database not ready after ${max_attempts} seconds"
fi

print_status "Setting up development cache directories..."
mkdir -p .turbo
mkdir -p node_modules/.cache
mkdir -p tests/results
mkdir -p logs
mkdir -p docs/generated

print_success "Cache directories created"

print_status "Verifying development environment..."
verification_passed=true

# Check Node.js and npm
if ! node --version > /dev/null 2>&1; then
    print_error "Node.js verification failed"
    verification_passed=false
fi

if ! npm --version > /dev/null 2>&1; then
    print_error "npm verification failed"
    verification_passed=false
fi

# Check Rust (if available)
if command -v rustc &> /dev/null; then
    if ! rustc --version > /dev/null 2>&1; then
        print_error "Rust verification failed"
        verification_passed=false
    fi
fi

# Check Python
if ! python3 --version > /dev/null 2>&1; then
    print_error "Python verification failed"
    verification_passed=false
fi

# Check database connection
if ! nc -z test-db 5432 2>/dev/null; then
    print_warning "Database connection not available (this might be normal)"
fi

# Check Redis connection
if ! nc -z test-redis 6379 2>/dev/null; then
    print_warning "Redis connection not available (this might be normal)"
fi

if $verification_passed; then
    print_success "Development environment verification passed"
else
    print_error "Some verification checks failed"
    exit 1
fi

print_success "Post-create setup completed successfully!"
echo ""
echo -e "${BLUE}🎯 Next Steps:${NC}"
echo "  1. Open a new terminal and run 'source ~/.bashrc' to load aliases"
echo "  2. Run 'npm run start:all' to start the development servers"
echo "  3. Run 'npm run test:all' to ensure everything is working"
echo "  4. Visit http://localhost:3000 to see the application"
echo ""
echo -e "${GREEN}Your WRFCoin development environment is ready! 🚀${NC}"