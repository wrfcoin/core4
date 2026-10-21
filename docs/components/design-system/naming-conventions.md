# WRFCoin Component Naming Conventions & Storybook Standards

## Component Naming Conventions

### File Naming Standards

1. **Component Files**: Use PascalCase for component files

   - ✅ `ComprehensiveDashboard.jsx`
   - ✅ `NetworkHealthMonitor.tsx`
   - ❌ `comprehensive-dashboard.jsx`
   - ❌ `network_health_monitor.tsx`

2. **Story Files**: Match component name with `.stories.ts` extension

   - ✅ `ComprehensiveDashboard.stories.ts`
   - ✅ `NetworkHealthMonitor.stories.ts`
   - ❌ `comprehensive-dashboard.stories.js`

3. **Directory Structure**: Organize by feature/domain
   ```
   src/components/
   ├── Dashboard/
   │   ├── ComprehensiveDashboard.jsx
   │   ├── NetworkStatus.jsx
   │   └── MetricsPanel.jsx
   ├── Governance/
   │   ├── ProposalList.jsx
   │   └── VotingInterface.jsx
   └── Common/
       ├── Button.jsx
       └── Modal.jsx
   ```

### Component Classification System

Based on the component analysis, we categorize components into:

#### 1. **Functional Components** (Type: `functional`)

- **Characteristics**: Pure presentation, no internal state
- **Examples**: `Button`, `Card`, `Icon`
- **Naming Pattern**: `[Purpose][Component]`
  - ✅ `PrimaryButton`, `SuccessCard`, `InfoIcon`
  - ❌ `ButtonPrimary`, `CardSuccess`, `IconInfo`

#### 2. **Hook-based Components** (Type: `hooks`)

- **Characteristics**: Use React hooks, manage state
- **Examples**: `SyncStatusWidget`, `NFTDetails`
- **Naming Pattern**: `[Feature][Component]`
  - ✅ `WalletConnector`, `DataLoader`, `StatusMonitor`
  - ❌ `ConnectorWallet`, `LoaderData`, `MonitorStatus`

#### 3. **Page Components** (Type: `page`)

- **Characteristics**: Route-level components, full page views
- **Examples**: `DashboardPage`, `GovernancePage`
- **Naming Pattern**: `[Feature]Page`
  - ✅ `WalletPage`, `SettingsPage`, `IntegrationsPage`
  - ❌ `PageWallet`, `PageSettings`, `PageIntegrations`

#### 4. **Container Components** (Type: `container`)

- **Characteristics**: Manage data and business logic
- **Examples**: `DashboardContainer`, `WalletContainer`
- **Naming Pattern**: `[Feature]Container`
  - ✅ `GovernanceContainer`, `MiningContainer`
  - ❌ `ContainerGovernance`, `ContainerMining`

## Storybook Story Naming Conventions

### Story Titles (Categories)

1. **Primary Categories**:

   ```typescript
   // Core interface components
   'Components/[ComponentName]';

   // Page-level components
   'Pages/[PageName]';

   // Dashboard widgets
   'Dashboard/[WidgetName]';

   // Form components
   'Forms/[FormName]';

   // Domain-specific
   'Governance/[ComponentName]';
   'Wallet/[ComponentName]';
   'Mining/[ComponentName]';
   'Bridge/[ComponentName]';
   'Analytics/[ComponentName]';
   ```

2. **Subcategories**: Use forward slashes for nested organization
   ```typescript
   'Dashboard/Widgets/NetworkStatus';
   'Forms/Inputs/TextInput';
   'Governance/Voting/ProposalCard';
   ```

### Story Variant Naming

1. **Standard Variants** (all components should have):

   - `Default` - Basic component state
   - `Interactive` - For interaction testing

2. **State-based Variants** (when applicable):

   - `Loading` - Loading state
   - `Error` - Error state
   - `Empty` - Empty/no data state
   - `Complete` - Fully populated state

3. **Functional Variants** (component-specific):
   - `Primary`, `Secondary` (for buttons)
   - `Mobile`, `Tablet`, `Desktop` (for responsive)
   - `Light`, `Dark` (for themes)

### Props and ArgTypes Standards

1. **Consistent Prop Names**:

   ```typescript
   // State props
   isLoading: boolean
   isDisabled: boolean
   isVisible: boolean

   // Handler props
   onClick: () => void
   onChange: (value: any) => void
   onSubmit: (data: any) => void

   // Data props
   data: any[]
   items: any[]
   value: any

   // Style props
   className: string
   variant: string
   size: 'small' | 'medium' | 'large'
   ```

2. **ArgTypes Configuration**:
   ```typescript
   argTypes: {
     variant: {
       control: 'select',
       options: ['primary', 'secondary', 'danger'],
       description: 'Visual variant of the component'
     },
     size: {
       control: 'radio',
       options: ['small', 'medium', 'large'],
       description: 'Size of the component'
     },
     isDisabled: {
       control: 'boolean',
       description: 'Whether the component is disabled'
     }
   }
   ```

## Component Quality Standards

### Complexity-based Requirements

#### **Low Complexity** (1-3)

- **Required Stories**: Default, Interactive
- **Testing**: Basic render test
- **Documentation**: Basic description

#### **Medium Complexity** (4-6)

- **Required Stories**: Default, Loading, Error, Interactive
- **Testing**: User interaction tests
- **Documentation**: Props documentation, usage examples

#### **High Complexity** (7+)

- **Required Stories**: Default, Loading, Error, Empty, Complete, Interactive
- **Testing**: Comprehensive interaction tests, accessibility tests
- **Documentation**: Full API documentation, multiple examples

### Documentation Standards

1. **Component Description**: Clear, concise explanation

   ```typescript
   docs: {
     description: {
       component: 'NetworkHealthMonitor displays real-time network metrics and status indicators for the WRFCoin blockchain network.';
     }
   }
   ```

2. **Story Descriptions**: Explain the specific use case
   ```typescript
   Error: {
     parameters: {
       docs: {
         description: {
           story: 'Shows the component when network connection fails or data cannot be loaded.';
         }
       }
     }
   }
   ```

## Template Selection Logic

### Automatic Template Assignment

```javascript
function selectTemplate(componentData) {
  const { name, type, complexity, filePath } = componentData;

  // Page components
  if (filePath.includes('/pages/') || name.endsWith('Page')) {
    return PAGE_COMPONENT_TEMPLATE;
  }

  // Dashboard components
  if (filePath.includes('/Dashboard/') || name.includes('Dashboard')) {
    return DASHBOARD_COMPONENT_TEMPLATE;
  }

  // Form components
  if (filePath.includes('/Form/') || name.includes('Form')) {
    return FORM_COMPONENT_TEMPLATE;
  }

  // Complex hook-based components
  if (type === 'hooks' || complexity >= 5) {
    return HOOKS_COMPONENT_TEMPLATE;
  }

  // Default functional components
  return FUNCTIONAL_COMPONENT_TEMPLATE;
}
```

## Migration Guidelines

### Updating Existing Stories

1. **Review Current Stories**: Check against naming conventions
2. **Update Titles**: Use new category structure
3. **Add Missing Variants**: Ensure all required stories exist
4. **Enhance Documentation**: Add proper descriptions and examples

### Priority Order for Updates

1. **High-Priority Components** (Main features):

   - `ComprehensiveDashboard`
   - `GovernanceDashboard`
   - `WalletPage`
   - `MiningDashboard`

2. **Common Components** (Reusable):

   - `Button`
   - `Card`
   - `Modal`
   - `Input`

3. **Integration Components** (External connections):
   - Bridge components
   - API integration components
   - Wallet connectors

### Example Migration

**Before:**

```typescript
// Old naming
title: 'auto-generated/Dashboard/ComprehensiveDashboard'

// Basic variants only
export const Default: Story = { ... }
```

**After:**

```typescript
// New naming
title: 'Dashboard/ComprehensiveDashboard'

// Complete variant set
export const Default: Story = { ... }
export const Loading: Story = { ... }
export const Error: Story = { ... }
export const LiveData: Story = { ... }
export const HighContrast: Story = { ... }
```

## Quality Metrics

### Success Criteria

- ✅ **Naming Consistency**: 100% of components follow naming conventions
- ✅ **Template Coverage**: All component types have appropriate templates
- ✅ **Story Completeness**: High-priority components have full story sets
- ✅ **Documentation Quality**: All stories have proper descriptions and examples
- ✅ **Accessibility**: Stories include accessibility testing variants
- ✅ **Responsiveness**: Page components include responsive variants

### Monitoring

- **Component Discovery**: 328+ components auto-discoverable
- **Story Generation**: Automated template-based generation
- **Quality Checks**: Automated validation of naming conventions
- **Documentation Coverage**: Track documentation completeness

---

## Quick Reference

### File Naming Checklist

- [ ] Component file uses PascalCase
- [ ] Story file matches component name + `.stories.ts`
- [ ] Directory follows feature organization
- [ ] Props use consistent naming patterns

### Story Checklist

- [ ] Title follows category/subcategory pattern
- [ ] Includes required variants for complexity level
- [ ] Has proper component description
- [ ] ArgTypes are properly configured
- [ ] Interactive testing is included
- [ ] Accessibility considerations are addressed

### Quality Checklist

- [ ] Component follows single responsibility principle
- [ ] Props are well-documented
- [ ] Error states are handled
- [ ] Loading states are implemented
- [ ] Responsive design is considered
- [ ] Accessibility standards are met
