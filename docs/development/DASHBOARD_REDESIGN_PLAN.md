# WRFcoin Main Dashboard Redesign Plan

## Overview

This document outlines the comprehensive redesign of the WRFcoin main dashboard, providing detailed visualization specifications, styling recommendations, and design inspiration for each section. The goal is to create a unified, intuitive experience that surfaces the platform's core value propositions: weather data monetization, cryptocurrency management, environmental impact, and community governance.

---

## Layout Structure

### Recommended Grid System
- **Desktop**: 12-column grid with 24px gutters
- **Tablet**: 8-column grid with 16px gutters  
- **Mobile**: 4-column single-column stack

### Section Priority (Top to Bottom)
1. Hero Stats Bar (always visible)
2. Weather & Environmental Hub
3. Crypto & Token Overview
4. Earnings & Banking
5. Network & Mining
6. Governance & Community
7. Gamification & Achievements
8. Additional Features

---

## Section 1: Hero Stats Bar

### Purpose
A persistent, compact horizontal bar displaying the most critical real-time metrics at a glance.

### Visualization
**Style**: Horizontal ticker/stats bar, similar to Bloomberg Terminal's top bar or CoinMarketCap's global stats ribbon.

**Layout**: 
- Fixed position at top of dashboard (below main navigation)
- 6-8 key metrics displayed inline with subtle separators
- Each metric shows: icon + label + value + trend indicator (up/down arrow with percentage)

**Suggested Metrics**:
| Metric | Icon | Format | Color Coding |
|--------|------|--------|--------------|
| WRF Price | Coin icon | $0.0847 (+2.4%) | Green/Red based on 24h change |
| Block Height | Cube icon | 1,247,893 | Neutral blue |
| Network TPS | Lightning bolt | 1,078 tx/s | Performance gradient |
| Active Nodes | Network icon | 647 nodes | Health indicator |
| Your Balance | Wallet icon | 12,450 WRF | User accent color |
| Weather Stations Active | Thermometer | 2 online | Green/Amber/Red status |

**Animation**: Subtle pulse animation when values update. Smooth counting animation for number changes (like odometer effect).

**Inspiration**:
- CoinMarketCap global stats bar
- TradingView ticker ribbon
- Bloomberg Terminal header metrics

---

## Section 2: Weather & Environmental Hub

### 2.1 Live Weather Map

#### Purpose
Interactive map showing weather station locations, current readings, and data contribution status.

#### Visualization
**Primary Display**: Full-width interactive map using Mapbox GL or Leaflet with custom styling.

**Map Layers** (toggleable):
1. **Station Markers**: Custom pin icons showing station type (AcuRite, Ambient Weather, etc.) with color-coded status
   - Green pulse: Active, transmitting
   - Amber: Online but stale data (>15 min old)
   - Red: Offline
   - Blue ring: Your stations (highlighted)

2. **Temperature Overlay**: Heat map gradient layer showing temperature readings across regions
   - Color scale: Deep blue (-20°F) → Cyan (32°F) → Green (50°F) → Yellow (70°F) → Orange (85°F) → Red (100°F+)

3. **Precipitation Layer**: Animated radar-style overlay with rain/snow intensity
   - Light blue dots for light rain
   - Blue wave patterns for moderate rain
   - Purple animated streaks for heavy precipitation
   - White crystalline patterns for snow

4. **Air Quality Layer**: Circular gradient zones around stations
   - AQI color scale: Green (0-50) → Yellow (51-100) → Orange (101-150) → Red (151-200) → Purple (201+)

5. **Data Contribution Density**: Heatmap showing areas with high/low data coverage
   - Helps users see where new stations would be valuable

**Map Controls**:
- Layer toggle panel (left sidebar, collapsible)
- Time slider to view historical data (last 24 hours)
- Zoom to my stations button
- Fullscreen toggle

**Popup on Station Click**:
- Station name and owner (anonymized if not user's)
- Current readings: Temp, Humidity, Pressure, Wind, Precipitation
- Data quality score (1-100)
- Earnings generated (if user's station)
- Last update timestamp

**Style**: Dark theme map tiles (similar to Mapbox Dark or custom "Night Mode" tiles) to make colored overlays pop. Subtle topographical lines for terrain context.

**Inspiration**:
- Windy.com (layer controls, animated weather)
- PurpleAir map (station markers with readings)
- Weather Underground Wundermap
- OpenWeatherMap layers

---

### 2.2 Personal Weather Station Cards

#### Purpose
Detailed cards for each of the user's connected weather stations.

#### Visualization
**Layout**: Horizontal scrolling carousel of cards (3 visible on desktop, 1 on mobile).

**Card Design** (each station):
```
┌─────────────────────────────────────────────┐
│  [Station Icon]  Bryan's AcuRite Station    │
│  ● Online | Last update: 2 min ago          │
├─────────────────────────────────────────────┤
│                                             │
│   72°F          45%           30.12 inHg    │
│   Temp          Humidity      Pressure      │
│   [sparkline]   [sparkline]   [sparkline]   │
│                                             │
│   8 mph NW      0.00"         Good          │
│   Wind          Rain Today    Air Quality   │
│                                             │
├─────────────────────────────────────────────┤
│  📊 Data Points Today: 1,247                │
│  💰 Earnings Today: 24.5 WRF                │
│  ⭐ Quality Score: 94/100                   │
└─────────────────────────────────────────────┘
```

**Sparklines**: Tiny inline charts (last 6 hours) for temperature, humidity, pressure showing trends without taking space.

**Color Coding**:
- Temperature: Gradient background hint (cool blue to warm orange)
- Quality Score: Green (90+), Amber (70-89), Red (<70)
- Earnings: Subtle gold accent

**Interaction**:
- Click to expand detailed view with full charts
- Quick action buttons: View History, Configure, Troubleshoot

**Inspiration**:
- Apple Weather app cards
- Netatmo dashboard
- Ambient Weather Network dashboard

---

### 2.3 Environmental Impact Score

#### Purpose
Gamified visualization of the user's environmental contribution.

#### Visualization
**Primary Element**: Large circular progress gauge (similar to Apple Watch activity rings).

**Design**:
```
         ╭───────────────╮
        ╱   ┌─────────┐   ╲
       │    │  GOLD   │    │
       │    │  TIER   │    │
       │    │  🌍     │    │
       │    │ 847 pts │    │
        ╲   └─────────┘   ╱
         ╰───────────────╯
    [Progress ring around center]
```

**Ring Layers** (3 concentric rings):
1. **Outer Ring (Green)**: Carbon offset contribution
2. **Middle Ring (Blue)**: Data quality score
3. **Inner Ring (Gold)**: Community participation

**Tier System Display**:
- Bronze → Silver → Gold → Platinum → Diamond
- Current tier prominently displayed in center
- Progress bar to next tier below

**Supporting Metrics** (below gauge):
- Total CO2 offset: "1.2 tonnes this year"
- Trees equivalent: "52 trees planted"
- Data points contributed: "847,293 readings"
- Ranking: "#127 of 12,450 contributors"

**Animation**: Rings animate on load, filling to current percentage with satisfying easing.

**Inspiration**:
- Apple Fitness rings
- Ecosia's tree counter
- Duolingo streak/XP displays
- Strava achievement badges

---

### 2.4 Air Quality Community Feed

#### Purpose
Real-time feed showing air quality data from the Sensor Community/Luftdaten network.

#### Visualization
**Layout**: Vertical scrolling feed with compact cards.

**Feed Item Design**:
```
┌────────────────────────────────────────┐
│ 📍 Downtown Austin, TX                 │
│ PM2.5: 12 μg/m³  |  PM10: 28 μg/m³    │
│ AQI: 48 (Good) 🟢                      │
│ Contributed by: sensor_24601          │
│ 3 min ago                              │
└────────────────────────────────────────┘
```

**Filter Options**:
- By region/distance from user
- By AQI level (show alerts only, all readings)
- By sensor type

**Alert Highlighting**: Cards with poor AQI get red left border and subtle red background tint.

**Inspiration**:
- Twitter/X feed layout
- IQAir community contributions
- PurpleAir community map sidebar

---

## Section 3: Crypto & Token Overview

### 3.1 Portfolio Value Card

#### Purpose
Primary display of user's WRFcoin holdings and total portfolio value.

#### Visualization
**Layout**: Large hero card spanning 8 columns (desktop).

**Design**:
```
┌──────────────────────────────────────────────────────────┐
│  Total Portfolio Value                                    │
│  $1,054.87                           ▲ +$24.50 (+2.4%)   │
│                                                          │
│  ┌────────────────────────────────────────────────────┐  │
│  │ [Area chart showing portfolio value over time]      │  │
│  │ Gradient fill: brand color to transparent           │  │
│  │ 7-day default, toggleable: 24h, 7d, 30d, 1y, All   │  │
│  └────────────────────────────────────────────────────┘  │
│                                                          │
│  Holdings Breakdown:                                      │
│  ├─ WRF: 12,450.00 ($1,012.50)      ████████████░░ 96%  │
│  ├─ Staked: 500.00 locked           ██░░░░░░░░░░░░  4%  │
│  └─ Pending Rewards: 12.34          (claimable)         │
└──────────────────────────────────────────────────────────┘
```

**Chart Style**:
- Smooth area chart with gradient fill (brand purple/blue at top, transparent at bottom)
- Subtle grid lines
- Crosshair tooltip on hover showing exact value and date
- Time range toggles below chart

**Color Scheme**:
- Positive change: Green (#22C55E)
- Negative change: Red (#EF4444)
- Neutral elements: Slate gray

**Animation**: Chart draws in smoothly on load. Numbers count up to final value.

**Inspiration**:
- Robinhood portfolio view
- Coinbase dashboard
- MetaMask portfolio

---

### 3.2 Price Chart Widget

#### Purpose
Focused WRFcoin price tracking with technical analysis light features.

#### Visualization
**Chart Type**: Candlestick or Line chart (user toggleable).

**Features**:
- Price display with 24h high/low
- Volume bars at bottom
- Simple moving average lines (7-day, 30-day)
- Support/resistance zones (subtle)

**Time Intervals**: 1H, 4H, 1D, 1W, 1M

**Design**:
```
┌────────────────────────────────────────────┐
│  WRF/USD           $0.0813 ▲ +1.2%        │
│  H: $0.0847  L: $0.0798  Vol: 2.4M        │
├────────────────────────────────────────────┤
│  [Candlestick/Line Chart]                  │
│                                            │
│  MA7 ── MA30 ──                           │
│                                            │
│  ▁▂▃▅▃▂▁▂▃▄▅▆▅▄▃▂ (volume bars)           │
├────────────────────────────────────────────┤
│  [1H] [4H] [1D] [1W] [1M]                 │
│  [📊 Line] [🕯️ Candle]                    │
└────────────────────────────────────────────┘
```

**Inspiration**:
- TradingView charts (simplified)
- Binance price widget
- CoinGecko coin pages

---

### 3.3 Recent Transactions List

#### Purpose
Quick view of latest account activity.

#### Visualization
**Layout**: Compact list with expandable rows.

**Row Design**:
```
┌────────────────────────────────────────────────────┐
│ ↗️ Sent      -250 WRF     0x7a3f...4e21    2h ago │
│ ↙️ Received  +500 WRF     Staking Reward   4h ago │
│ 🔄 Swap      100 WRF→ETH  Uniswap          1d ago │
│ 🌤️ Earned    +24.5 WRF   Weather Data     1d ago │
└────────────────────────────────────────────────────┘
```

**Transaction Type Icons**:
- ↗️ Sent (red accent)
- ↙️ Received (green accent)
- 🔄 Swap (blue accent)
- 🌤️ Weather earnings (gold accent)
- ⚡ Mining reward (purple accent)
- 🗳️ Governance (teal accent)

**Expandable Details**: Click row to show full transaction hash, gas fees, block number, confirmations.

**Inspiration**:
- Etherscan transaction list
- Apple Wallet transaction history
- Venmo activity feed

---

### 3.4 Quick Action Buttons

#### Purpose
One-click access to common crypto operations.

#### Visualization
**Layout**: Horizontal button group below portfolio card.

**Buttons**:
```
┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐
│  ↗️  │ │  ↙️  │ │  🔄  │ │  🌉  │ │  📥  │
│ Send │ │Receive│ │ Swap │ │Bridge│ │ Buy  │
└──────┘ └──────┘ └──────┘ └──────┘ └──────┘
```

**Style**:
- Subtle background, prominent icon
- Hover: Background fill with brand color
- Active: Slight scale down

**Inspiration**:
- Phantom wallet quick actions
- Cash App home screen
- Venmo pay buttons

---

## Section 4: Earnings & Banking

### 4.1 Earnings Dashboard Card

#### Purpose
Comprehensive view of all income streams from the WRFcoin platform.

#### Visualization
**Layout**: Two-column card with summary left, breakdown right.

**Left Column (Summary)**:
```
┌─────────────────────────┐
│  Total Earnings         │
│  This Month             │
│                         │
│  847.25 WRF             │
│  ≈ $68.94 USD           │
│                         │
│  ▲ +12% vs last month   │
│                         │
│  [Bar chart: 6-month    │
│   earnings trend]       │
└─────────────────────────┘
```

**Right Column (Breakdown by Source)**:
```
┌─────────────────────────────────┐
│  Earnings by Source             │
│                                 │
│  🌤️ Weather Data    542 WRF    │
│     ███████████████░░░░  64%    │
│                                 │
│  ⚡ Mining           189 WRF    │
│     ██████░░░░░░░░░░░░░  22%    │
│                                 │
│  📊 Staking          98 WRF    │
│     ████░░░░░░░░░░░░░░░  12%    │
│                                 │
│  🌉 Bridge Fees      18 WRF    │
│     █░░░░░░░░░░░░░░░░░░   2%    │
└─────────────────────────────────┘
```

**Chart Type**: Horizontal stacked bar or donut chart for source breakdown.

**Color Coding by Source**:
- Weather: Sky blue (#0EA5E9)
- Mining: Purple (#A855F7)
- Staking: Green (#22C55E)
- Bridge: Orange (#F97316)
- Other: Gray (#64748B)

**Inspiration**:
- Stripe dashboard revenue breakdown
- QuickBooks income summary
- Mint income categorization

---

### 4.2 Pending Payouts & Claims

#### Purpose
Show all claimable rewards with easy claim actions.

#### Visualization
**Layout**: Alert-style cards for each pending payout.

**Card Design**:
```
┌────────────────────────────────────────────────┐
│ 🎁 Staking Rewards Available                   │
│                                                │
│ 45.67 WRF ready to claim                       │
│ Accumulated over 7 days                        │
│                                                │
│ [Claim Now] [Auto-compound]                    │
└────────────────────────────────────────────────┘
```

**Multiple Payout Types**:
- Staking rewards (weekly claim)
- Weather data earnings (daily)
- Governance participation rewards
- Referral bonuses
- Competition winnings

**Urgency Indicators**:
- Green: Ready to claim
- Amber: Claim deadline approaching
- Pulsing animation for high-value claims

**Inspiration**:
- DeFi protocol claim interfaces (Aave, Compound)
- Airline miles redemption portals
- Credit card rewards dashboards

---

### 4.3 Earnings Calendar/History

#### Purpose
Historical view of earnings with calendar visualization.

#### Visualization
**Primary View**: Calendar heatmap (GitHub contribution style).

**Design**:
```
┌──────────────────────────────────────────────────────────┐
│  Earnings Activity - Last 52 Weeks                       │
│                                                          │
│  Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec         │
│  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░   │
│  ░░▓▓░░░░░░░░░░░░██░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░   │
│  ░░▓▓░░░░░░░░░░░░██░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░   │
│  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░   │
│  Mon Wed Fri                                             │
│                                                          │
│  Less ░░░▒▒▓▓██ More                                    │
└──────────────────────────────────────────────────────────┘
```

**Intensity Colors**: Light to dark gradient based on daily earnings amount.

**Hover Tooltip**: Shows exact date and earnings breakdown.

**Below Calendar**: Scrollable list of transactions for selected date.

**Inspiration**:
- GitHub contribution graph
- Apple Health activity calendar
- Strava training log

---

## Section 5: Network & Mining

### 5.1 Node Status Panel

#### Purpose
Real-time health monitoring for user's connected nodes.

#### Visualization
**Layout**: Grid of node cards (2-3 per row).

**Node Card Design**:
```
┌──────────────────────────────────┐
│  🖥️ Full Validator Node          │
│  node-east-01.wrfcoin.net       │
│                                  │
│  ● Online    Synced: 100%        │
│                                  │
│  CPU ████████░░ 78%              │
│  RAM █████░░░░░ 52%              │
│  NET ██████░░░░ 64%              │
│                                  │
│  Blocks Validated: 12,847        │
│  Uptime: 99.97% (45d)           │
│                                  │
│  [Details] [Restart] [Logs]      │
└──────────────────────────────────┘
```

**Status Indicators**:
- Green dot + "Online": Healthy
- Amber dot + "Syncing": Catching up
- Red dot + "Offline": Connection lost
- Blue dot + "Maintenance": User-initiated pause

**Resource Bars**: Gradient fills (green → amber → red as utilization increases).

**Inspiration**:
- DigitalOcean droplet dashboard
- AWS EC2 instance list
- Grafana server panels

---

### 5.2 Mining Performance Chart

#### Purpose
Visualize mining output and efficiency over time.

#### Visualization
**Chart Type**: Dual-axis area chart.

**Left Axis**: Hash rate (TH/s)  
**Right Axis**: Blocks found

**Design**:
```
┌────────────────────────────────────────────────────────┐
│  Mining Performance - Last 30 Days                     │
│                                                        │
│  1.2 TH/s │        ╭──╮                          │ 15  │
│  0.8 TH/s │    ╭──╯    ╰──╮    ╭────╮            │ 10  │
│  0.4 TH/s │╭──╯            ╰──╯      ╰────       │  5  │
│  0 TH/s   │────────────────────────────────      │  0  │
│            Jan 1                        Jan 30        │
│                                                        │
│  ── Hash Rate (avg: 0.89 TH/s)                        │
│  ▓▓ Blocks Found (total: 127)                         │
└────────────────────────────────────────────────────────┘
```

**Supporting Stats Below**:
- Total blocks found
- Estimated earnings this period
- Efficiency rating vs network average
- Luck factor (actual vs expected blocks)

**Inspiration**:
- NiceHash dashboard
- F2Pool mining stats
- Ethermine worker graphs

---

### 5.3 Network Health Overview

#### Purpose
Show overall WRFcoin network status and user's contribution.

#### Visualization
**Layout**: Gauge cluster with supporting metrics.

**Primary Gauge**: Network health score (0-100)

**Secondary Metrics** (cards below):
```
┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
│  647        │ │  1,078      │ │  99.98%     │ │  3.2s       │
│  Total Nodes│ │  TPS        │ │  Uptime     │ │  Block Time │
│  ▲ +12      │ │  ▲ +5%      │ │  ── stable  │ │  ▼ -0.1s    │
└─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘
```

**Your Contribution Highlight**:
- "You provide 0.14% of network hash power"
- "Your nodes validated 0.8% of today's blocks"

**Network Map** (optional): Small world map with node density dots.

**Inspiration**:
- Solana Beach network stats
- Ethereum network dashboards
- Bitcoin network explorers

---

## Section 6: Governance & Community

### 6.1 Active Proposals Widget

#### Purpose
Surface governance opportunities requiring user attention.

#### Visualization
**Layout**: Card list with urgency sorting.

**Proposal Card**:
```
┌────────────────────────────────────────────────────────┐
│  🗳️ WIP-042: Increase Weather Data Rewards by 15%     │
│                                                        │
│  Status: Active Voting    Ends in: 2d 14h             │
│                                                        │
│  Current Results:                                      │
│  For     ████████████████░░░░░░░░  67.3% (1.2M WRF)   │
│  Against ████████░░░░░░░░░░░░░░░░  32.7% (584K WRF)   │
│                                                        │
│  Quorum: ██████████████░░░░░░░░░░ 71% reached         │
│                                                        │
│  [View Details] [Vote For] [Vote Against]              │
└────────────────────────────────────────────────────────┘
```

**Urgency Indicators**:
- Red banner: "Ending soon! Vote now"
- Gold banner: "High impact proposal"
- Blue banner: "Your stake qualifies you to vote"

**Vote History**: Collapsed section showing user's past votes.

**Inspiration**:
- Snapshot.org proposal pages
- Compound governance
- MakerDAO voting dashboard

---

### 6.2 Voting Power Display

#### Purpose
Show user's governance influence and how to increase it.

#### Visualization
**Design**:
```
┌────────────────────────────────────────┐
│  Your Voting Power                     │
│                                        │
│  ⚡ 12,450 WRF                         │
│                                        │
│  Ranking: #847 of 12,450 voters        │
│  Top 7% of governance participants     │
│                                        │
│  Power Breakdown:                      │
│  ├─ Held tokens: 11,950 WRF           │
│  ├─ Staked bonus: +500 WRF            │
│  └─ Delegation received: 0 WRF        │
│                                        │
│  [Delegate Power] [Increase Stake]     │
└────────────────────────────────────────┘
```

**Visual Element**: Shield or badge icon that grows/changes with voting power tier.

**Inspiration**:
- ENS DAO voting power
- Gitcoin passport
- Uniswap governance

---

### 6.3 Community Announcements Feed

#### Purpose
Official updates and community highlights.

#### Visualization
**Layout**: Twitter-style feed with pinned important items.

**Feed Types**:
- 📢 Official Announcements (pinned, highlighted)
- 🏆 Community Achievements ("User X reached 1M data points!")
- 🗳️ Governance Results ("WIP-041 passed with 78% approval")
- 🔧 Maintenance Notices
- 📰 Weekly Digests

**Design**:
```
┌────────────────────────────────────────────────────────┐
│ 📌 PINNED                                              │
│ 📢 WRFcoin v2.1 Released - New Features!              │
│    Read about improved weather data validation...      │
│    2 hours ago · Official                              │
├────────────────────────────────────────────────────────┤
│ 🏆 Community Milestone                                 │
│    We've reached 10 million validated weather readings!│
│    Thank you to all contributors.                      │
│    5 hours ago                                         │
├────────────────────────────────────────────────────────┤
│ 🗳️ Governance Update                                  │
│    WIP-041 "Carbon Credit Integration" passed         │
│    Implementation begins next week.                    │
│    1 day ago                                           │
└────────────────────────────────────────────────────────┘
```

**Inspiration**:
- Discord announcement channels
- Medium publication feeds
- Notion team updates

---

## Section 7: Gamification & Achievements

### 7.1 Achievement Badges Gallery

#### Purpose
Display unlocked achievements and progress toward new ones.

#### Visualization
**Layout**: Horizontal scrollable badge gallery with categories.

**Badge Design**:
```
     ╭─────╮
    ╱  ⛈️   ╲     Weather Warrior
   │  GOLD  │     Collected 100,000
   │  ★★★★  │     weather data points
    ╲       ╱     
     ╰─────╯      Unlocked: Jan 5, 2025
```

**Badge States**:
- Full color + glow: Unlocked
- Grayscale + lock icon: Locked (show progress bar below)
- Golden shimmer: Rare/special

**Categories**:
- Weather Data (🌤️ icons)
- Network Participation (🖥️ icons)
- Governance (🗳️ icons)
- Environmental (🌍 icons)
- Social/Community (👥 icons)
- Special Events (⭐ icons)

**Progress Tracking**:
```
┌────────────────────────────────────────┐
│  🔒 Data Deity (Locked)                │
│  Contribute 1 million data points      │
│                                        │
│  Progress: 847,293 / 1,000,000        │
│  ████████████████░░░░░  84.7%          │
│                                        │
│  Est. unlock: ~18 days                 │
└────────────────────────────────────────┘
```

**Inspiration**:
- Xbox/PlayStation achievement system
- Duolingo badges
- Stack Overflow reputation badges
- Strava trophies

---

### 7.2 Leaderboard Widget

#### Purpose
Competitive element showing user ranking across categories.

#### Visualization
**Layout**: Tabbed leaderboard with multiple categories.

**Tabs**: All Time | This Month | This Week

**Categories Dropdown**: Weather Data | Mining | Governance | Overall

**Design**:
```
┌────────────────────────────────────────────────────────┐
│  🏆 Weather Data Leaderboard - This Month             │
│                                                        │
│  Rank  User           Data Points    Earnings         │
│  ──────────────────────────────────────────────       │
│  🥇 1   weather_king   1,247,893      847 WRF         │
│  🥈 2   storm_chaser   1,102,456      723 WRF         │
│  🥉 3   rain_master     987,234       651 WRF         │
│     4   cloud_nine      876,543       578 WRF         │
│     5   sunny_days      765,432       505 WRF         │
│  ──────────────────────────────────────────────       │
│  → 127  you (bryan_wx)   124,567       82 WRF        │
│                                        ▲ +15 ranks    │
└────────────────────────────────────────────────────────┘
```

**User Highlight**: User's own row highlighted with accent color, shows rank change.

**Anonymity Options**: Toggle to show/hide usernames (privacy).

**Inspiration**:
- Fortnite/gaming leaderboards
- Peloton class rankings
- Fantasy sports standings

---

### 7.3 Daily/Weekly Challenges

#### Purpose
Short-term goals to drive engagement.

#### Visualization
**Layout**: Compact challenge cards with timer.

**Design**:
```
┌────────────────────────────────────────────────────────┐
│  ⏰ Daily Challenges           Resets in: 6h 24m      │
├────────────────────────────────────────────────────────┤
│  ✅ Submit 100 weather readings     +5 WRF   [Done]   │
│  ▶️ Validate 10 community data      +3 WRF   [8/10]   │
│  ○  Vote on 1 governance proposal   +10 WRF  [0/1]    │
├────────────────────────────────────────────────────────┤
│  Streak: 🔥 12 days    Bonus: +15% rewards            │
└────────────────────────────────────────────────────────┘
```

**Streak Mechanic**: Fire emoji with count, bonus multiplier display.

**Weekly Challenge Banner**:
- Larger featured challenge with bigger rewards
- Progress bar with community goal

**Inspiration**:
- Duolingo daily quests
- Fortnite battle pass challenges
- Apple Fitness rings and challenges

---

## Section 8: Additional Features

### 8.1 NFT Spotlight Carousel

#### Purpose
Showcase user's computational NFTs and collectibles.

#### Visualization
**Layout**: 3D card carousel with auto-rotation.

**Card Design**:
```
┌──────────────────────────────────┐
│  [NFT Image/Animation]           │
│                                  │
│  Weather Oracle #4521            │
│  Legendary | Computational       │
│                                  │
│  Computing Power: 2.4 TH/s       │
│  Earnings: 12.5 WRF/day         │
│                                  │
│  [View] [Stake] [Transfer]       │
└──────────────────────────────────┘
```

**NFT Categories**:
- Computational NFTs (with hash power display)
- Weather Data NFTs (historical storm data, etc.)
- Governance NFTs (voting power boosters)
- Collectible art

**Animation**: Cards tilt on hover (3D effect), auto-rotate through collection.

**Inspiration**:
- OpenSea collection display
- Rarible NFT cards
- NBA Top Shot moments

---

### 8.2 Cross-Chain Bridge Status

#### Purpose
Track assets bridged to/from other networks.

#### Visualization
**Layout**: Network diagram with active bridges.

**Design**:
```
                    ┌─────────┐
                    │Ethereum │
                    │ 500 WRF │
                    └────┬────┘
                         │
           ┌─────────────┼─────────────┐
           │             │             │
      ┌────┴────┐   ┌────┴────┐   ┌────┴────┐
      │ Solana  │   │  WRF   │   │ Polygon │
      │ 200 WRF │   │ 12,450 │   │ 100 WRF │
      └─────────┘   │ (Home) │   └─────────┘
                    └─────────┘
```

**Pending Transfers List**:
```
┌────────────────────────────────────────────────────────┐
│  ⏳ Pending Bridge Transfers                           │
│                                                        │
│  ETH → WRF    250 WRF    Confirming (12/24 blocks)    │
│  WRF → SOL    100 WRF    Processing...                │
└────────────────────────────────────────────────────────┘
```

**Inspiration**:
- LayerSwap bridge interface
- Wormhole bridge dashboard
- Hop Protocol

---

### 8.3 AI Insights Widget (Cortex AI)

#### Purpose
Personalized recommendations powered by platform AI.

#### Visualization
**Layout**: Chat-style insights with actionable buttons.

**Design**:
```
┌────────────────────────────────────────────────────────┐
│  🤖 Cortex AI Insights                                │
├────────────────────────────────────────────────────────┤
│  💡 "Based on your weather data patterns, adding a    │
│      rain sensor could increase your earnings by      │
│      an estimated 23%."                               │
│                                             [Learn More]│
├────────────────────────────────────────────────────────┤
│  📊 "You have 45.67 WRF in unclaimed staking rewards. │
│      Claim now before the weekly reset."              │
│                                           [Claim Now] │
├────────────────────────────────────────────────────────┤
│  🗳️ "A proposal you might be interested in: WIP-042  │
│      about weather data rewards ends in 2 days."      │
│                                             [View Now] │
└────────────────────────────────────────────────────────┘
```

**Insight Types**:
- Optimization suggestions
- Reminder alerts
- Opportunity notifications
- Market insights
- Security alerts

**Inspiration**:
- Google Assistant suggestions
- Mint financial insights
- Coinbase learning prompts

---

### 8.4 Integration Status Panel

#### Purpose
Show health of external integrations (Golem, iExec, IoTeX).

#### Visualization
**Layout**: Compact status indicators.

**Design**:
```
┌────────────────────────────────────────────────────────┐
│  🔌 Partner Integrations                              │
│                                                        │
│  Golem Network      ● Connected     2.4 TH/s rented   │
│  iExec              ● Connected     5 tasks active    │
│  IoTeX              ● Connected     3 devices linked  │
│  Sensor.Community   ● Connected     1,247 readings    │
│  Chainlink Oracle   ● Connected     Price: $0.0813    │
│                                                        │
│  [Manage Integrations]                                 │
└────────────────────────────────────────────────────────┘
```

**Status Colors**:
- Green dot: Connected, healthy
- Amber dot: Connected, degraded
- Red dot: Disconnected
- Gray dot: Not configured

**Inspiration**:
- Zapier connected apps
- IFTTT service status
- GitHub integrations page

---

### 8.5 Carbon Credit Portfolio

#### Purpose
Track accumulated environmental credits.

#### Visualization
**Layout**: Portfolio-style card with credit types.

**Design**:
```
┌────────────────────────────────────────────────────────┐
│  🌱 Carbon Credit Portfolio                           │
│                                                        │
│  Total Credits: 127.5 tCO2e                           │
│  Estimated Value: $2,550 USD                          │
│                                                        │
│  Credit Breakdown:                                     │
│  ├─ Verified (Gold Standard): 85.2 tCO2e    ███████   │
│  ├─ Pending Verification: 32.1 tCO2e        ███       │
│  └─ Retired/Used: 10.2 tCO2e                █         │
│                                                        │
│  [View Certificates] [Retire Credits] [Sell/Trade]    │
└────────────────────────────────────────────────────────┘
```

**Visual Elements**:
- Tree growth animation based on credits
- Verification badges (Gold Standard, Verra, etc.)
- Timeline of credit accumulation

**Inspiration**:
- Patch carbon credit dashboard
- Klima DAO portfolio
- Nori carbon removal marketplace

---

## Global Design Specifications

### Color Palette

**Primary Colors**:
- Brand Blue: #3B82F6
- Brand Purple: #8B5CF6
- Success Green: #22C55E
- Warning Amber: #F59E0B
- Error Red: #EF4444

**Neutral Colors**:
- Background Dark: #0F172A (Slate 900)
- Card Background: #1E293B (Slate 800)
- Border: #334155 (Slate 700)
- Text Primary: #F8FAFC (Slate 50)
- Text Secondary: #94A3B8 (Slate 400)

**Accent Colors** (for data categories):
- Weather: #0EA5E9 (Sky 500)
- Mining: #A855F7 (Purple 500)
- Staking: #22C55E (Green 500)
- Governance: #14B8A6 (Teal 500)
- Environmental: #84CC16 (Lime 500)

### Typography

**Font Family**: Inter (sans-serif) for UI, JetBrains Mono for data/numbers

**Scale**:
- Hero numbers: 48px, bold
- Section headers: 24px, semibold
- Card titles: 18px, semibold
- Body text: 14px, regular
- Captions/labels: 12px, medium

### Card Styling

**Default Card**:
```css
{
  background: rgba(30, 41, 59, 0.8);
  backdrop-filter: blur(12px);
  border: 1px solid rgba(51, 65, 85, 0.5);
  border-radius: 16px;
  padding: 24px;
  box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
}
```

**Hover State**:
```css
{
  border-color: rgba(59, 130, 246, 0.5);
  box-shadow: 0 0 20px rgba(59, 130, 246, 0.1);
  transform: translateY(-2px);
}
```

### Animation Guidelines

**Principles**:
- Use subtle, purposeful animations
- Duration: 150-300ms for micro-interactions, 300-500ms for transitions
- Easing: ease-out for entrances, ease-in for exits

**Common Animations**:
- Card hover: translateY(-2px) with shadow increase
- Number changes: Counting animation (odometer style)
- Charts: Draw-in on load
- Status indicators: Subtle pulse for active states
- Notifications: Slide in from right

### Accessibility

- Minimum contrast ratio: 4.5:1 for text
- Focus indicators: Visible ring on all interactive elements
- Screen reader labels: All icons have aria-labels
- Keyboard navigation: Full tab support
- Color blind safe: Don't rely solely on color for status

### Responsive Breakpoints

- Desktop: 1280px+
- Tablet: 768px - 1279px
- Mobile: < 768px

---

## Implementation Priority

### Phase 1 (MVP)
1. Hero Stats Bar
2. Portfolio Value Card
3. Weather Station Cards
4. Quick Action Buttons
5. Recent Transactions

### Phase 2 (Core Features)
6. Live Weather Map
7. Earnings Dashboard
8. Node Status Panel
9. Active Proposals Widget
10. Achievement Badges

### Phase 3 (Enhanced Experience)
11. Price Chart Widget
12. Mining Performance Chart
13. Leaderboard Widget
14. Daily Challenges
15. Community Feed

### Phase 4 (Advanced Features)
16. NFT Spotlight
17. Cross-Chain Bridge Status
18. AI Insights Widget
19. Carbon Credit Portfolio
20. Integration Status Panel

---

## Design Resources & Inspiration Links

### Weather/Data Visualization
- Windy.com (layer controls, animations)
- Weather Underground (station network)
- PurpleAir (community sensors)
- Netatmo (personal weather stations)

### Crypto Dashboards
- Zapper.fi (portfolio aggregation)
- DeBank (multi-chain view)
- Zerion (clean portfolio design)
- Rainbow Wallet (mobile-first design)

### Financial Dashboards
- Robinhood (simplicity)
- Mint (categorization)
- Personal Capital (wealth tracking)
- Stripe Dashboard (merchant analytics)

### Gamification
- Duolingo (streaks, XP, leagues)
- Strava (achievements, leaderboards)
- Xbox Achievements (badge design)
- Peloton (competitive features)

### Network/Infrastructure
- Grafana (monitoring dashboards)
- Datadog (service health)
- DigitalOcean (simple server management)
- Vercel (deployment status)

---

*Document Version: 1.0*
*Created: January 2026*
*For: WRFcoin Dashboard Redesign Project*
