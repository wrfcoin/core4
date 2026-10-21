# MkDocs Setup Guide for WRFCoin Platform

This guide provides instructions for setting up MkDocs to generate the comprehensive architecture
handbook from the WRFCoin documentation.

## Prerequisites

MkDocs requires Python 3.7+ and pip to be installed on your system.

### Installing Python and pip

#### Ubuntu/Debian:

```bash
sudo apt-get update
sudo apt-get install python3 python3-pip
```

#### macOS:

```bash
brew install python
```

#### Windows:

Download Python from [python.org](https://python.org) and ensure pip is included.

## Installing MkDocs and Plugins

Once Python and pip are available, install MkDocs and the required plugins:

```bash
# Install MkDocs and essential plugins
pip3 install mkdocs mkdocs-material mkdocs-mermaid2-plugin mkdocs-awesome-pages-plugin

# Verify installation
mkdocs --version
```

## Building the Architecture Handbook

### Option 1: Using npm scripts (Recommended)

```bash
# Build the architecture handbook
npm run docs:handbook

# Build and serve with live reload
npm run docs:handbook:serve

# Watch for changes during development
npm run docs:handbook:watch
```

### Option 2: Using MkDocs directly

```bash
# Build static documentation
mkdocs build --site-dir docs/generated/handbook --clean

# Serve with live reload during development
mkdocs serve --dev-addr=127.0.0.1:8001
```

### Option 3: Using the build script

```bash
# Build only
bash scripts/build-mkdocs.sh

# Build and serve
bash scripts/build-mkdocs.sh --serve
```

## Configuration

The MkDocs configuration is defined in `mkdocs.yml` at the project root. Key features include:

### Theme and Styling

- **Material Design**: Clean, responsive documentation theme
- **Custom Branding**: WRFCoin colors and styling in `docs/assets/stylesheets/extra.css`
- **Dark/Light Mode**: Automatic theme switching based on user preference

### Enhanced Features

- **Mermaid Diagrams**: Interactive system architecture diagrams
- **Code Highlighting**: Syntax highlighting for multiple languages
- **Search**: Full-text search with suggestions
- **Navigation**: Hierarchical navigation with breadcrumbs
- **Mobile Responsive**: Optimized for all device sizes

### Content Organization

The handbook includes comprehensive coverage of:

- **Architecture Overview**: System design and component relationships
- **Development Guides**: Setup, workflows, and contribution guidelines
- **API Reference**: Complete API documentation with examples
- **SDK Documentation**: Multi-platform SDK guides
- **Security**: Protocols, compliance, and best practices
- **Tutorials**: Step-by-step learning paths
- **Specifications**: Technical specifications and standards

## Output

The generated documentation will be available at:

- **Local Development**: `http://127.0.0.1:8001`
- **Built Files**: `docs/generated/handbook/index.html`

## Integration with CI/CD

The MkDocs build is integrated into the main documentation pipeline:

```bash
# Build all documentation (includes MkDocs handbook)
npm run docs:build

# Deploy all documentation
npm run docs:deploy
```

## Customization

### Adding New Sections

1. Add markdown files to the `docs/` directory
2. Update the `nav` section in `mkdocs.yml`
3. Rebuild the documentation

### Modifying Styling

- Edit `docs/assets/stylesheets/extra.css` for custom styles
- Edit `docs/assets/javascripts/extra.js` for enhanced functionality
- Customize `mkdocs.yml` for theme configuration

### Adding Plugins

1. Install the plugin: `pip3 install mkdocs-plugin-name`
2. Add to the `plugins` section in `mkdocs.yml`
3. Configure plugin settings as needed

## Troubleshooting

### Common Issues

**MkDocs not found:**

- Ensure Python 3.7+ is installed
- Install pip: `sudo apt-get install python3-pip`
- Verify PATH includes pip installation directory

**Plugin errors:**

- Install missing plugins: `pip3 install [plugin-name]`
- Check plugin compatibility with MkDocs version

**Build failures:**

- Check for broken internal links in markdown files
- Verify all referenced files exist
- Review `mkdocs.yml` syntax

**Styling issues:**

- Clear browser cache
- Check CSS syntax in `extra.css`
- Verify custom JavaScript in `extra.js`

### Performance Optimization

For large documentation sites:

```yaml
# In mkdocs.yml
plugins:
  - search:
      indexing: 'full'
  - awesome-pages:
      filename: .pages
      collapse_single_pages: true
```

## Architecture Decision Records (ADRs)

The handbook automatically includes existing architectural documentation:

- Architecture overviews and diagrams
- Technical specifications and standards
- Implementation summaries and reports
- Security and compliance documentation
- Development guides and workflows

## Maintenance

### Regular Updates

1. Update documentation as features are added
2. Review and update architectural diagrams
3. Validate internal and external links
4. Update version information and changelog

### Content Review

- Quarterly review of all documentation sections
- Update screenshots and examples
- Verify code samples and configurations
- Check for outdated information

---

The MkDocs architecture handbook provides a comprehensive, searchable, and professional
documentation portal that serves as the definitive guide to the WRFCoin platform's architecture and
implementation.
