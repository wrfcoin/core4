// WRFCoin Platform Documentation - Enhanced JavaScript Functionality

document.addEventListener('DOMContentLoaded', function () {
  // Initialize enhanced documentation features
  initializeCodeCopyButtons();
  initializeTooltips();
  initializeProgressIndicators();
  initializeSearchEnhancements();
  initializeNavigationEnhancements();
  initializeMermaidDiagrams();
  initializeVersionNotifications();
  initializeAnalytics();
});

/**
 * Enhanced code copy functionality with feedback
 */
function initializeCodeCopyButtons() {
  document.querySelectorAll('.md-clipboard').forEach(button => {
    button.addEventListener('click', function () {
      // Add visual feedback
      const originalText = this.title;
      this.title = 'Copied!';
      this.style.color = '#00cc66';

      setTimeout(() => {
        this.title = originalText;
        this.style.color = '';
      }, 2000);
    });
  });
}

/**
 * Enhanced tooltips for technical terms
 */
function initializeTooltips() {
  const technicalTerms = {
    HSM: 'Hardware Security Module - A dedicated crypto processor for managing digital keys',
    AMM: 'Automated Market Maker - A type of decentralized exchange protocol',
    DeFi: 'Decentralized Finance - Financial services using smart contracts',
    NFT: 'Non-Fungible Token - Unique digital assets on blockchain',
    DAO: 'Decentralized Autonomous Organization - Community-governed entity',
    IPFS: 'InterPlanetary File System - Distributed file storage protocol',
    GraphQL: 'Graph Query Language - API query language and runtime',
    JWT: 'JSON Web Token - Secure token format for authentication',
    CORS: 'Cross-Origin Resource Sharing - Web security feature',
    API: 'Application Programming Interface - Software interaction protocol',
  };

  Object.entries(technicalTerms).forEach(([term, definition]) => {
    const regex = new RegExp(`\\b${term}\\b`, 'gi');
    document
      .querySelectorAll('.md-content p, .md-content li')
      .forEach(element => {
        if (element.innerHTML.match(regex) && !element.querySelector('a')) {
          element.innerHTML = element.innerHTML.replace(
            regex,
            `<span class="tooltip" title="${definition}">${term}</span>`
          );
        }
      });
  });

  // Add tooltip styling
  const style = document.createElement('style');
  style.textContent = `
        .tooltip {
            position: relative;
            border-bottom: 1px dotted var(--wrfcoin-primary);
            cursor: help;
        }
        
        .tooltip:hover::after {
            content: attr(title);
            position: absolute;
            bottom: 100%;
            left: 50%;
            transform: translateX(-50%);
            background: var(--wrfcoin-dark);
            color: white;
            padding: 0.5rem;
            border-radius: 4px;
            font-size: 0.8rem;
            white-space: nowrap;
            z-index: 1000;
            box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2);
        }
        
        .tooltip:hover::before {
            content: '';
            position: absolute;
            bottom: 100%;
            left: 50%;
            transform: translateX(-50%);
            border: 5px solid transparent;
            border-top-color: var(--wrfcoin-dark);
            z-index: 1000;
        }
    `;
  document.head.appendChild(style);
}

/**
 * Progress indicators for multi-step tutorials
 */
function initializeProgressIndicators() {
  document.querySelectorAll('.tutorial-steps').forEach(container => {
    const steps = container.querySelectorAll('.step');
    const indicator = document.createElement('div');
    indicator.className = 'progress-indicator';

    steps.forEach((step, index) => {
      const dot = document.createElement('div');
      dot.className = 'progress-dot';
      if (index === 0) dot.classList.add('active');
      indicator.appendChild(dot);
    });

    container.insertBefore(indicator, container.firstChild);
  });
}

/**
 * Enhanced search with suggestions
 */
function initializeSearchEnhancements() {
  const searchInput = document.querySelector('.md-search__input');
  if (!searchInput) return;

  // Add search suggestions
  const suggestions = [
    'Getting Started',
    'API Reference',
    'SDK Documentation',
    'Security',
    'Smart Contracts',
    'Cross-Chain',
    'DeFi',
    'NFT',
    'Governance',
    'Architecture',
    'Development',
    'Testing',
    'Deployment',
  ];

  searchInput.addEventListener('focus', function () {
    if (!this.value) {
      showSearchSuggestions(suggestions);
    }
  });

  function showSearchSuggestions(items) {
    const existing = document.querySelector('.search-suggestions');
    if (existing) existing.remove();

    const container = document.createElement('div');
    container.className = 'search-suggestions';
    container.style.cssText = `
            position: absolute;
            top: 100%;
            left: 0;
            right: 0;
            background: white;
            border: 1px solid rgba(0, 102, 204, 0.2);
            border-radius: 4px;
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
            z-index: 1000;
            max-height: 200px;
            overflow-y: auto;
        `;

    items.slice(0, 6).forEach(item => {
      const suggestion = document.createElement('div');
      suggestion.textContent = item;
      suggestion.style.cssText = `
                padding: 0.5rem 1rem;
                cursor: pointer;
                border-bottom: 1px solid rgba(0, 102, 204, 0.1);
            `;
      suggestion.addEventListener('click', () => {
        searchInput.value = item;
        container.remove();
        searchInput.dispatchEvent(new Event('input'));
      });
      container.appendChild(suggestion);
    });

    searchInput.parentElement.style.position = 'relative';
    searchInput.parentElement.appendChild(container);

    // Remove suggestions when clicking outside
    document.addEventListener(
      'click',
      function (e) {
        if (!searchInput.contains(e.target) && !container.contains(e.target)) {
          container.remove();
        }
      },
      { once: true }
    );
  }
}

/**
 * Navigation enhancements
 */
function initializeNavigationEnhancements() {
  // Add reading progress indicator
  const content = document.querySelector('.md-content');
  if (!content) return;

  const progressBar = document.createElement('div');
  progressBar.style.cssText = `
        position: fixed;
        top: 0;
        left: 0;
        width: 0%;
        height: 3px;
        background: var(--wrfcoin-accent);
        z-index: 1000;
        transition: width 0.3s ease;
    `;
  document.body.appendChild(progressBar);

  window.addEventListener('scroll', () => {
    const scrolled =
      (window.scrollY /
        (document.documentElement.scrollHeight - window.innerHeight)) *
      100;
    progressBar.style.width = Math.min(scrolled, 100) + '%';
  });

  // Add "back to top" button
  const backToTop = document.createElement('button');
  backToTop.innerHTML = '↑';
  backToTop.style.cssText = `
        position: fixed;
        bottom: 2rem;
        right: 2rem;
        width: 3rem;
        height: 3rem;
        border-radius: 50%;
        background: var(--wrfcoin-primary);
        color: white;
        border: none;
        font-size: 1.2rem;
        cursor: pointer;
        opacity: 0;
        transition: opacity 0.3s ease;
        z-index: 1000;
        box-shadow: 0 4px 12px rgba(0, 102, 204, 0.3);
    `;
  document.body.appendChild(backToTop);

  backToTop.addEventListener('click', () => {
    window.scrollTo({ top: 0, behavior: 'smooth' });
  });

  window.addEventListener('scroll', () => {
    backToTop.style.opacity = window.scrollY > 300 ? '1' : '0';
  });

  // Add breadcrumb navigation
  addBreadcrumbs();
}

/**
 * Add breadcrumb navigation
 */
function addBreadcrumbs() {
  const content = document.querySelector('.md-content__inner');
  if (!content) return;

  const breadcrumbs = document.createElement('nav');
  breadcrumbs.className = 'breadcrumbs';
  breadcrumbs.style.cssText = `
        padding: 1rem 0;
        margin-bottom: 1rem;
        border-bottom: 1px solid rgba(0, 102, 204, 0.1);
        font-size: 0.9rem;
    `;

  const path = window.location.pathname.split('/').filter(Boolean);
  const items = ['Home'];
  let currentPath = '';

  path.forEach(segment => {
    currentPath += '/' + segment;
    const title = segment
      .replace(/[-_]/g, ' ')
      .replace(/\b\w/g, l => l.toUpperCase());
    items.push(title);
  });

  breadcrumbs.innerHTML = items
    .map((item, index) => {
      if (index === items.length - 1) {
        return `<span style="color: var(--wrfcoin-gray);">${item}</span>`;
      }
      return `<a href="#" style="color: var(--wrfcoin-primary); text-decoration: none;">${item}</a>`;
    })
    .join(
      ' <span style="color: var(--wrfcoin-gray); margin: 0 0.5rem;">›</span> '
    );

  content.insertBefore(breadcrumbs, content.firstChild);
}

/**
 * Enhanced Mermaid diagram support
 */
function initializeMermaidDiagrams() {
  if (typeof mermaid !== 'undefined') {
    mermaid.initialize({
      theme: 'base',
      themeVariables: {
        primaryColor: '#0066cc',
        primaryTextColor: '#ffffff',
        primaryBorderColor: '#004499',
        lineColor: '#666666',
        secondaryColor: '#00ccff',
        tertiaryColor: '#f8f9fa',
      },
    });

    // Add click handlers for interactive diagrams
    document.querySelectorAll('.mermaid').forEach(diagram => {
      diagram.addEventListener('click', function (event) {
        if (
          event.target.tagName === 'rect' ||
          event.target.tagName === 'text'
        ) {
          const nodeText =
            event.target.textContent || event.target.getAttribute('data-node');
          if (nodeText) {
            console.log('Diagram node clicked:', nodeText);
            // Could navigate to related documentation
          }
        }
      });
    });
  }
}

/**
 * Version update notifications
 */
function initializeVersionNotifications() {
  // Check for documentation updates
  const lastVisit = localStorage.getItem('wrfcoin-docs-last-visit');
  const currentTime = Date.now();

  if (
    !lastVisit ||
    currentTime - parseInt(lastVisit) > 7 * 24 * 60 * 60 * 1000
  ) {
    showUpdateNotification();
  }

  localStorage.setItem('wrfcoin-docs-last-visit', currentTime.toString());
}

function showUpdateNotification() {
  const notification = document.createElement('div');
  notification.style.cssText = `
        position: fixed;
        top: 1rem;
        right: 1rem;
        background: var(--wrfcoin-primary);
        color: white;
        padding: 1rem;
        border-radius: 8px;
        box-shadow: 0 4px 12px rgba(0, 102, 204, 0.3);
        z-index: 1000;
        max-width: 300px;
        font-size: 0.9rem;
    `;

  notification.innerHTML = `
        <strong>📚 Documentation Updated!</strong><br>
        New features and improvements have been added to the WRFCoin platform documentation.
        <button onclick="this.parentElement.remove()" style="
            background: rgba(255, 255, 255, 0.2);
            border: none;
            color: white;
            padding: 0.25rem 0.5rem;
            border-radius: 4px;
            margin-left: 0.5rem;
            cursor: pointer;
        ">✕</button>
    `;

  document.body.appendChild(notification);

  // Auto-remove after 10 seconds
  setTimeout(() => {
    if (notification.parentElement) {
      notification.remove();
    }
  }, 10000);
}

/**
 * Basic analytics for documentation usage
 */
function initializeAnalytics() {
  // Track page views and popular sections
  const pageData = {
    url: window.location.pathname,
    title: document.title,
    timestamp: Date.now(),
    referrer: document.referrer,
  };

  // Store in localStorage for basic analytics
  const analytics = JSON.parse(
    localStorage.getItem('wrfcoin-docs-analytics') || '[]'
  );
  analytics.push(pageData);

  // Keep only last 100 entries
  if (analytics.length > 100) {
    analytics.splice(0, analytics.length - 100);
  }

  localStorage.setItem('wrfcoin-docs-analytics', JSON.stringify(analytics));

  // Track time spent on page
  let startTime = Date.now();

  window.addEventListener('beforeunload', () => {
    const timeSpent = Date.now() - startTime;
    const timeData = {
      url: window.location.pathname,
      timeSpent: timeSpent,
      timestamp: Date.now(),
    };

    const timeAnalytics = JSON.parse(
      localStorage.getItem('wrfcoin-docs-time') || '[]'
    );
    timeAnalytics.push(timeData);

    if (timeAnalytics.length > 100) {
      timeAnalytics.splice(0, timeAnalytics.length - 100);
    }

    localStorage.setItem('wrfcoin-docs-time', JSON.stringify(timeAnalytics));
  });
}

/**
 * Keyboard shortcuts for navigation
 */
document.addEventListener('keydown', function (event) {
  // Only trigger if not in input fields
  if (event.target.tagName === 'INPUT' || event.target.tagName === 'TEXTAREA') {
    return;
  }

  switch (event.key) {
    case '/':
      event.preventDefault();
      const searchInput = document.querySelector('.md-search__input');
      if (searchInput) {
        searchInput.focus();
      }
      break;
    case 'g':
      if (event.ctrlKey || event.metaKey) {
        event.preventDefault();
        window.location.href = '/';
      }
      break;
    case 'Escape':
      // Close any open modals or suggestions
      document
        .querySelectorAll('.search-suggestions')
        .forEach(el => el.remove());
      break;
  }
});

/**
 * Add keyboard shortcut help
 */
function addKeyboardShortcutHelp() {
  const helpButton = document.createElement('button');
  helpButton.innerHTML = '?';
  helpButton.title = 'Keyboard Shortcuts';
  helpButton.style.cssText = `
        position: fixed;
        bottom: 2rem;
        left: 2rem;
        width: 2.5rem;
        height: 2.5rem;
        border-radius: 50%;
        background: rgba(0, 102, 204, 0.8);
        color: white;
        border: none;
        font-size: 1rem;
        cursor: pointer;
        z-index: 1000;
        opacity: 0.7;
        transition: opacity 0.3s ease;
    `;

  helpButton.addEventListener('click', () => {
    alert(`Keyboard Shortcuts:
        
/ - Focus search
Ctrl/Cmd + G - Go to home
Esc - Close modals
        
Navigation:
← → - Previous/Next page
↑ ↓ - Scroll up/down`);
  });

  helpButton.addEventListener('mouseenter', () => {
    helpButton.style.opacity = '1';
  });

  helpButton.addEventListener('mouseleave', () => {
    helpButton.style.opacity = '0.7';
  });

  document.body.appendChild(helpButton);
}

// Initialize keyboard shortcuts help
addKeyboardShortcutHelp();
