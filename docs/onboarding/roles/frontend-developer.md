# Frontend Developer Onboarding Path

Welcome to frontend development on the WRFCoin Platform! This guide will get you up and running with
our React/TypeScript codebase.

## Path Overview

**Estimated Time**: 2-3 hours  
**Difficulty**: Intermediate  
**Prerequisites**: React, TypeScript, modern JavaScript

## Phase 1: Environment Setup (45 minutes)

### Frontend-Specific Requirements

```bash
# Verify Node.js version (20+)
node --version

# Frontend-specific tools
npm install -g @storybook/cli
npm install -g lighthouse
npm install -g @playwright/test
```

### Frontend Development Tools

**Required VS Code Extensions:**

```bash
# Install via VS Code or command line
code --install-extension ms-vscode.vscode-typescript-next
code --install-extension bradlc.vscode-tailwindcss
code --install-extension ms-playwright.playwright
code --install-extension chrmarti.regex
code --install-extension usernamehw.errorlens
```

**Additional Tools:**

- React Developer Tools (browser extension)
- Redux DevTools (browser extension)
- Accessibility Insights (browser extension)

### Project Structure Familiarization

```bash
# Navigate to frontend directory
cd frontend/

# Explore key directories
ls -la src/
echo "Key frontend directories:"
echo "- src/components/     # Reusable UI components"
echo "- src/pages/         # Page-level components"
echo "- src/hooks/         # Custom React hooks"
echo "- src/services/      # API integration"
echo "- src/stores/        # State management"
echo "- src/styles/        # Global styles and themes"
echo "- src/types/         # TypeScript type definitions"
echo "- src/utils/         # Helper functions"
```

### Setup Verification

```bash
# Start frontend development server
npm run dev:frontend

# In another terminal, run tests
npm run test:frontend

# Start Storybook
npm run storybook

# Run linting
npm run lint:frontend
```

**Verification Checklist:**

- [ ] Development server starts on http://localhost:3000
- [ ] Storybook starts on http://localhost:6006
- [ ] All tests pass
- [ ] No linting errors
- [ ] Hot reload works when editing files

## Phase 2: Technology Stack Overview (30 minutes)

### Core Technologies

**React Ecosystem:**

```typescript
// React 18 with Concurrent Features
import { Suspense, lazy } from 'react';
import { createRoot } from 'react-dom/client';

// TypeScript for type safety
interface WeatherData {
  temperature: number;
  humidity: number;
  pressure: number;
  location: string;
}

// Modern hooks and patterns
import { useState, useEffect, useMemo, useCallback } from 'react';
```

**State Management:**

```typescript
// Zustand for simple state management
import { create } from 'zustand';

interface WeatherStore {
  weatherData: WeatherData[];
  loading: boolean;
  fetchWeatherData: () => Promise<void>;
}

// React Query for server state
import { useQuery, useMutation } from '@tanstack/react-query';
```

**Styling Solutions:**

```tsx
// Tailwind CSS for utility-first styling
<div className="bg-blue-500 text-white p-4 rounded-lg shadow-md">

// Styled Components for complex styling
import styled from 'styled-components';

const WeatherCard = styled.div`
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
  border-radius: 12px;
  padding: 1.5rem;
`;
```

### Architecture Patterns

**Component Architecture:**

```
components/
├── ui/                 # Basic UI components (Button, Input, etc.)
├── weather/           # Weather-specific components
├── trading/           # Carbon trading components
├── nft/              # Computational NFT components
├── layout/           # Layout components (Header, Sidebar)
└── forms/            # Form components and validations
```

**Data Flow Patterns:**

```typescript
// API Layer
export const weatherApi = {
  getCurrentWeather: (location: string) =>
    fetch(`/api/weather/current?location=${location}`),

  getForecast: (location: string, days: number) =>
    fetch(`/api/weather/forecast?location=${location}&days=${days}`)
};

// Custom Hooks
export const useWeatherData = (location: string) => {
  return useQuery({
    queryKey: ['weather', location],
    queryFn: () => weatherApi.getCurrentWeather(location),
    refetchInterval: 300000 // 5 minutes
  });
};

// Component Usage
export const WeatherWidget: React.FC<{ location: string }> = ({ location }) => {
  const { data, isLoading, error } = useWeatherData(location);

  if (isLoading) return <WeatherSkeleton />;
  if (error) return <ErrorAlert error={error} />;

  return <WeatherDisplay data={data} />;
};
```

## Phase 3: Development Workflow (45 minutes)

### Component Development Process

**1. Create Component Structure:**

```bash
# Use component generator
npm run generate:component WeatherDashboard

# This creates:
# - src/components/weather/WeatherDashboard/
#   ├── index.ts
#   ├── WeatherDashboard.tsx
#   ├── WeatherDashboard.test.tsx
#   ├── WeatherDashboard.stories.tsx
#   └── WeatherDashboard.module.css
```

**2. Component Development:**

```typescript
// WeatherDashboard.tsx
import React from 'react';
import { useWeatherData } from '../../hooks/useWeatherData';
import { WeatherCard } from '../WeatherCard';
import { LoadingSpinner } from '../ui/LoadingSpinner';

interface WeatherDashboardProps {
  locations: string[];
  refreshInterval?: number;
  onLocationSelect?: (location: string) => void;
}

export const WeatherDashboard: React.FC<WeatherDashboardProps> = ({
  locations,
  refreshInterval = 300000,
  onLocationSelect
}) => {
  const { data, isLoading, error } = useWeatherData(locations, refreshInterval);

  return (
    <div className="weather-dashboard grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
      {isLoading && <LoadingSpinner />}
      {error && <ErrorAlert message={error.message} />}
      {data?.map((weather) => (
        <WeatherCard
          key={weather.location}
          data={weather}
          onClick={() => onLocationSelect?.(weather.location)}
        />
      ))}
    </div>
  );
};
```

**3. Storybook Development:**

```typescript
// WeatherDashboard.stories.tsx
import type { Meta, StoryObj } from '@storybook/react';
import { WeatherDashboard } from './WeatherDashboard';

const meta: Meta<typeof WeatherDashboard> = {
  title: 'Weather/WeatherDashboard',
  component: WeatherDashboard,
  parameters: {
    layout: 'fullscreen',
  },
  argTypes: {
    onLocationSelect: { action: 'location selected' },
  },
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    locations: ['Tokyo', 'New York', 'London'],
  },
};

export const Loading: Story = {
  args: {
    locations: ['Tokyo'],
  },
  parameters: {
    msw: {
      handlers: [
        // Mock loading state
      ],
    },
  },
};
```

**4. Testing:**

```typescript
// WeatherDashboard.test.tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { WeatherDashboard } from './WeatherDashboard';

const createTestQueryClient = () => new QueryClient({
  defaultOptions: {
    queries: { retry: false },
    mutations: { retry: false },
  },
});

const renderWithProviders = (component: React.ReactElement) => {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      {component}
    </QueryClientProvider>
  );
};

describe('WeatherDashboard', () => {
  it('renders loading state initially', () => {
    renderWithProviders(
      <WeatherDashboard locations={['Tokyo']} />
    );

    expect(screen.getByRole('progressbar')).toBeInTheDocument();
  });

  it('calls onLocationSelect when weather card is clicked', async () => {
    const mockOnLocationSelect = jest.fn();

    renderWithProviders(
      <WeatherDashboard
        locations={['Tokyo']}
        onLocationSelect={mockOnLocationSelect}
      />
    );

    // Wait for data to load and click
    const weatherCard = await screen.findByText('Tokyo');
    fireEvent.click(weatherCard);

    expect(mockOnLocationSelect).toHaveBeenCalledWith('Tokyo');
  });
});
```

### Development Commands

```bash
# Development server with hot reload
npm run dev:frontend

# Run specific test suites
npm run test:frontend -- --watch
npm run test:frontend -- --coverage

# Storybook development
npm run storybook
npm run build-storybook

# Type checking
npm run type-check:frontend

# Linting and formatting
npm run lint:frontend
npm run lint:frontend:fix
npm run format:frontend

# Build for production
npm run build:frontend
npm run preview:frontend
```

## Phase 4: Platform Integration (60 minutes)

### API Integration Patterns

**GraphQL Integration:**

```typescript
// Generated GraphQL hooks
import { useGetWeatherDataQuery, useCreateForecastMutation } from '../generated/graphql';

export const WeatherComponent = () => {
  const { data, loading, error } = useGetWeatherDataQuery({
    variables: { location: 'Tokyo' }
  });

  const [createForecast] = useCreateForecastMutation({
    onCompleted: (data) => {
      // Handle success
    },
    onError: (error) => {
      // Handle error
    }
  });

  return (
    // Component JSX
  );
};
```

**REST API Integration:**

```typescript
// API client configuration
import { createApiClient } from '../services/api';

const apiClient = createApiClient({
  baseURL: process.env.NEXT_PUBLIC_API_URL,
  timeout: 10000,
  retries: 3,
});

// Custom hooks for API calls
export const useWeatherForecast = (location: string) => {
  return useQuery({
    queryKey: ['forecast', location],
    queryFn: async () => {
      const response = await apiClient.get(`/weather/forecast/${location}`);
      return response.data;
    },
    staleTime: 5 * 60 * 1000, // 5 minutes
    cacheTime: 30 * 60 * 1000, // 30 minutes
  });
};
```

### Blockchain Integration

**Wallet Connection:**

```typescript
// Web3 integration
import { useWallet } from '../hooks/useWallet';
import { useContract } from '../hooks/useContract';

export const CarbonTradingWidget = () => {
  const { address, isConnected, connect, disconnect } = useWallet();
  const { contract, loading } = useContract('CarbonCredit');

  const handleTrade = async (amount: number) => {
    if (!contract) return;

    try {
      const tx = await contract.trade(amount);
      await tx.wait();
      // Handle success
    } catch (error) {
      // Handle error
    }
  };

  return (
    <div>
      {!isConnected ? (
        <button onClick={connect}>Connect Wallet</button>
      ) : (
        <TradingInterface onTrade={handleTrade} />
      )}
    </div>
  );
};
```

**Smart Contract Interaction:**

```typescript
// Contract hooks
export const useComputationalNFT = () => {
  const { contract } = useContract('ComputationalNFT');

  const mintNFT = useMutation({
    mutationFn: async ({ modelType, parameters }: MintParams) => {
      const tx = await contract.mint(modelType, parameters);
      return tx.wait();
    },
  });

  const getNFTData = useQuery({
    queryKey: ['nft-data'],
    queryFn: async () => {
      return contract.getAllNFTs();
    },
  });

  return { mintNFT, getNFTData };
};
```

### Real-Time Features

**WebSocket Integration:**

```typescript
// Real-time weather updates
import { useWebSocket } from '../hooks/useWebSocket';

export const RealTimeWeather = ({ location }: { location: string }) => {
  const { data, connectionStatus } = useWebSocket(`/ws/weather/${location}`, {
    onMessage: (data) => {
      // Handle real-time weather updates
    },
    onError: (error) => {
      console.error('WebSocket error:', error);
    }
  });

  return (
    <div>
      <div>Status: {connectionStatus}</div>
      <WeatherDisplay data={data} />
    </div>
  );
};
```

## Phase 5: Best Practices & Standards (30 minutes)

### Code Quality Standards

**TypeScript Best Practices:**

```typescript
// Strict typing
interface WeatherApiResponse {
  readonly temperature: number;
  readonly humidity: number;
  readonly location: string;
  readonly timestamp: Date;
}

// Utility types
type RequiredWeatherFields = Required<Pick<WeatherData, 'temperature' | 'location'>>;
type PartialWeatherUpdate = Partial<WeatherData>;

// Generic components
interface DataTableProps<T> {
  data: T[];
  columns: ColumnDefinition<T>[];
  onRowClick?: (item: T) => void;
}

export const DataTable = <T extends Record<string, any>>({
  data,
  columns,
  onRowClick,
}: DataTableProps<T>) => {
  // Implementation
};
```

**Performance Optimization:**

```typescript
// Memoization patterns
const WeatherChart = React.memo(({ data }: { data: WeatherData[] }) => {
  const chartData = useMemo(() => {
    return processWeatherData(data);
  }, [data]);

  const handleDataPointClick = useCallback((point: DataPoint) => {
    // Handle click
  }, []);

  return <Chart data={chartData} onPointClick={handleDataPointClick} />;
});

// Lazy loading
const HeavyWeatherAnalytics = lazy(() => import('./HeavyWeatherAnalytics'));

export const WeatherPage = () => (
  <Suspense fallback={<LoadingSpinner />}>
    <HeavyWeatherAnalytics />
  </Suspense>
);
```

**Accessibility Standards:**

```typescript
// ARIA labels and semantic HTML
export const WeatherAlert = ({ level, message }: AlertProps) => (
  <div
    role="alert"
    aria-live="polite"
    className={`alert alert-${level}`}
    aria-label={`${level} weather alert`}
  >
    <h3 id="alert-title">Weather Alert</h3>
    <p aria-describedby="alert-title">{message}</p>
  </div>
);

// Keyboard navigation
export const InteractiveMap = () => {
  const handleKeyDown = (event: KeyboardEvent) => {
    switch (event.key) {
      case 'ArrowUp':
        // Move selection up
        break;
      case 'Enter':
      case ' ':
        // Activate selection
        break;
    }
  };

  return (
    <div
      role="application"
      aria-label="Interactive weather map"
      tabIndex={0}
      onKeyDown={handleKeyDown}
    >
      {/* Map content */}
    </div>
  );
};
```

## Phase 6: First Contribution (30 minutes)

### Practice Exercise

**Task**: Create a simple weather card component

1. **Create Component:**

   ```bash
   npm run generate:component SimpleWeatherCard
   ```

2. **Implement Basic Functionality:**

   ```typescript
   // Display temperature, location, and weather icon
   // Add hover effects and responsive design
   // Include accessibility features
   ```

3. **Write Tests:**

   ```typescript
   // Test rendering with different weather data
   // Test accessibility features
   // Test responsive behavior
   ```

4. **Create Storybook Story:**

   ```typescript
   // Multiple states (sunny, rainy, cloudy)
   // Different data scenarios
   // Interactive controls
   ```

5. **Submit Pull Request:**
   ```bash
   git checkout -b feature/simple-weather-card
   git add .
   git commit -m "feat: add SimpleWeatherCard component"
   git push origin feature/simple-weather-card
   ```

### Verification Checklist

- [ ] Component renders correctly
- [ ] All tests pass
- [ ] Storybook story works
- [ ] Accessibility tests pass
- [ ] TypeScript compiles without errors
- [ ] Follows coding standards
- [ ] PR template completed

## Next Steps

### Advanced Topics

- [ ] Complex state management patterns
- [ ] Advanced animation techniques
- [ ] Performance optimization strategies
- [ ] Advanced testing patterns
- [ ] Micro-frontend architecture

### Specialization Paths

- [ ] **UI/UX Specialist**: Design system development
- [ ] **Performance Engineer**: Bundle optimization, loading strategies
- [ ] **Accessibility Expert**: WCAG compliance, assistive technologies
- [ ] **Mobile Developer**: React Native integration

### Resources

- [Frontend Architecture Guide](../../guides/development/frontend-architecture.md)
- [Component Library Documentation](../../frontend/components/README.md)
- [Performance Guidelines](../../guides/development/performance-guidelines.md)
- [Accessibility Guidelines](../../guides/development/accessibility-guidelines.md)

**Congratulations!** You're now ready to contribute to WRFCoin's frontend development.

**Questions?** Join [#frontend-dev](https://discord.gg/wrfcoin-frontend) on Discord!

---

**Last Updated**: January 2025 **Estimated Completion**: 2-3 hours **Difficulty**: Intermediate
