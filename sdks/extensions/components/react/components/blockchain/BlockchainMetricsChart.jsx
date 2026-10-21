import React, { useState, useEffect } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
  AreaChart,
  Area,
  BarChart,
  Bar,
  ComposedChart,
} from 'recharts';

/**
 * BlockchainMetricsChart component for visualizing Wrfcoin blockchain metrics
 *
 * This component displays various blockchain metrics like block height,
 * hashrate, difficulty, and transaction count over time.
 */
const BlockchainMetricsChart = ({
  metric = 'blockHeight',
  timeRange = '24h',
  chartType = 'line',
  height = 300,
  theme = 'light',
  aggregation = 'hourly',
  showEvents = false,
  customColors,
  onDataPointClick,
  refreshInterval = 60000,
}) => {
  const { sdk } = useWrfcoinContext();
  const [data, setData] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [networkInfo, setNetworkInfo] = useState({});

  // Generate timeframe in milliseconds based on timeRange
  const getTimeframeMs = range => {
    switch (range) {
      case '1h':
        return 60 * 60 * 1000;
      case '6h':
        return 6 * 60 * 60 * 1000;
      case '12h':
        return 12 * 60 * 60 * 1000;
      case '24h':
        return 24 * 60 * 60 * 1000;
      case '7d':
        return 7 * 24 * 60 * 60 * 1000;
      case '30d':
        return 30 * 24 * 60 * 60 * 1000;
      case '90d':
        return 90 * 24 * 60 * 60 * 1000;
      case '1y':
        return 365 * 24 * 60 * 60 * 1000;
      default:
        return 24 * 60 * 60 * 1000;
    }
  };

  // Get chart settings based on metric type
  const getChartSettings = type => {
    const baseSettings = {
      yAxisLabel: '',
      dataKey: 'value',
      unit: '',
      color: '#3a8eff',
      domain: [0, 'auto'],
    };

    switch (type) {
      case 'blockHeight':
        return {
          ...baseSettings,
          yAxisLabel: 'Block Height',
          unit: '',
          color: customColors?.blockHeight || '#2196f3',
          domain: [0, 'auto'],
        };
      case 'hashrate':
        return {
          ...baseSettings,
          yAxisLabel: 'Hashrate',
          unit: 'H/s',
          color: customColors?.hashrate || '#4caf50',
          domain: [0, 'auto'],
        };
      case 'difficulty':
        return {
          ...baseSettings,
          yAxisLabel: 'Difficulty',
          unit: '',
          color: customColors?.difficulty || '#ff9800',
          domain: [0, 'auto'],
        };
      case 'transactions':
        return {
          ...baseSettings,
          yAxisLabel: 'Transactions',
          unit: '/block',
          color: customColors?.transactions || '#9c27b0',
          domain: [0, 'auto'],
        };
      case 'fees':
        return {
          ...baseSettings,
          yAxisLabel: 'Transaction Fees',
          unit: 'WRF',
          color: customColors?.fees || '#f44336',
          domain: [0, 'auto'],
        };
      case 'blockTime':
        return {
          ...baseSettings,
          yAxisLabel: 'Block Time',
          unit: 's',
          color: customColors?.blockTime || '#673ab7',
          domain: [0, 'auto'],
        };
      case 'blockSize':
        return {
          ...baseSettings,
          yAxisLabel: 'Block Size',
          unit: 'KB',
          color: customColors?.blockSize || '#00bcd4',
          domain: [0, 'auto'],
        };
      case 'activeNodes':
        return {
          ...baseSettings,
          yAxisLabel: 'Active Nodes',
          unit: '',
          color: customColors?.activeNodes || '#8bc34a',
          domain: [0, 'auto'],
        };
      default:
        return baseSettings;
    }
  };

  // Fetch blockchain metrics
  useEffect(() => {
    if (!sdk) return;

    const fetchBlockchainMetrics = async () => {
      try {
        setLoading(true);
        setError(null);

        const timeframeMs = getTimeframeMs(timeRange);
        const now = new Date();
        const startTime = new Date(now.getTime() - timeframeMs);

        // Fetch blockchain metrics
        const metricsData = await sdk.getBlockchainMetrics({
          metric: metric,
          startTime: startTime.toISOString(),
          endTime: now.toISOString(),
          aggregation: aggregation,
          includeEvents: showEvents,
        });

        if (metricsData && metricsData.data) {
          setData(metricsData.data);

          // Set network info
          if (metricsData.networkInfo) {
            setNetworkInfo(metricsData.networkInfo);
          }
        } else {
          setData([]);
          setError('No data available for the selected parameters');
        }
      } catch (err) {
        console.error('Error fetching blockchain metrics:', err);
        setError('Failed to load blockchain metrics. Please try again later.');
      } finally {
        setLoading(false);
      }
    };

    // Fetch data immediately
    fetchBlockchainMetrics();

    // Set up interval for refreshing data
    const intervalId = setInterval(fetchBlockchainMetrics, refreshInterval);

    return () => {
      clearInterval(intervalId);
    };
  }, [sdk, metric, timeRange, aggregation, showEvents, refreshInterval]);

  // Format timestamp for display
  const formatTimestamp = timestamp => {
    if (!timestamp) return '';

    const date = new Date(timestamp);
    const options = { hour: '2-digit', minute: '2-digit' };

    // Use different format based on time range
    if (
      timeRange === '7d' ||
      timeRange === '30d' ||
      timeRange === '90d' ||
      timeRange === '1y'
    ) {
      return date.toLocaleDateString();
    }

    return date.toLocaleTimeString(undefined, options);
  };

  // Format value for display
  const formatValue = (value, unit) => {
    if (value === undefined || value === null) return 'N/A';

    // Format large numbers with appropriate suffixes
    if (metric === 'hashrate' && value > 1000000) {
      return `${(value / 1000000).toFixed(2)} MH/s`;
    } else if (metric === 'hashrate' && value > 1000) {
      return `${(value / 1000).toFixed(2)} KH/s`;
    }

    return `${value}${unit ? ` ${unit}` : ''}`;
  };

  // Handle data point click
  const handleDataPointClick = (data, index) => {
    if (onDataPointClick && typeof onDataPointClick === 'function') {
      onDataPointClick(data, index);
    }
  };

  // Get formatted events for display on chart
  const getFormattedEvents = events => {
    if (!events || !Array.isArray(events) || events.length === 0) {
      return [];
    }

    return events.map(event => ({
      ...event,
      icon: getEventIcon(event.type),
      color: getEventColor(event.type),
    }));
  };

  // Get event icon based on type
  const getEventIcon = type => {
    switch (type) {
      case 'hardfork':
        return '🔱';
      case 'upgrade':
        return '⬆️';
      case 'networkIssue':
        return '⚠️';
      case 'governance':
        return '📝';
      default:
        return '📌';
    }
  };

  // Get event color based on type
  const getEventColor = type => {
    switch (type) {
      case 'hardfork':
        return '#ff9800';
      case 'upgrade':
        return '#4caf50';
      case 'networkIssue':
        return '#f44336';
      case 'governance':
        return '#2196f3';
      default:
        return '#9e9e9e';
    }
  };

  // Get chart component based on chart type
  const getChartComponent = () => {
    const chartSettings = getChartSettings(metric);

    switch (chartType) {
      case 'line':
        return (
          <LineChart
            data={data}
            margin={{ top: 5, right: 30, left: 20, bottom: 5 }}
          >
            <CartesianGrid
              strokeDasharray="3 3"
              stroke={theme === 'dark' ? '#444' : '#eee'}
            />
            <XAxis
              dataKey="timestamp"
              tickFormatter={formatTimestamp}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
            />
            <YAxis
              label={{
                value: chartSettings.yAxisLabel,
                angle: -90,
                position: 'insideLeft',
                fill: theme === 'dark' ? '#ddd' : '#666',
              }}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
              domain={chartSettings.domain}
            />
            <Tooltip
              formatter={value => [
                formatValue(value, chartSettings.unit),
                chartSettings.yAxisLabel,
              ]}
              labelFormatter={timestamp => formatTimestamp(timestamp)}
              contentStyle={{
                backgroundColor: theme === 'dark' ? '#333' : '#fff',
                border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
                color: theme === 'dark' ? '#fff' : '#333',
              }}
            />
            <Legend />
            <Line
              type="monotone"
              dataKey={chartSettings.dataKey}
              stroke={chartSettings.color}
              activeDot={{ onClick: handleDataPointClick, r: 8 }}
              name={chartSettings.yAxisLabel}
              dot={{ r: 3 }}
            />

            {showEvents &&
              data[0]?.events &&
              data.map((point, index) => {
                if (!point.events || point.events.length === 0) return null;

                return getFormattedEvents(point.events).map(
                  (event, eventIndex) => (
                    <Line
                      key={`event-${index}-${eventIndex}`}
                      type="none"
                      dataKey={() => point.value}
                      stroke="none"
                      activeDot={{ r: 0 }}
                      name={event.type}
                      dot={{
                        r: 10,
                        cx: 0,
                        cy: 0,
                        fill: event.color,
                        stroke: 'none',
                      }}
                    />
                  )
                );
              })}
          </LineChart>
        );

      case 'area':
        return (
          <AreaChart
            data={data}
            margin={{ top: 5, right: 30, left: 20, bottom: 5 }}
          >
            <CartesianGrid
              strokeDasharray="3 3"
              stroke={theme === 'dark' ? '#444' : '#eee'}
            />
            <XAxis
              dataKey="timestamp"
              tickFormatter={formatTimestamp}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
            />
            <YAxis
              label={{
                value: chartSettings.yAxisLabel,
                angle: -90,
                position: 'insideLeft',
                fill: theme === 'dark' ? '#ddd' : '#666',
              }}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
              domain={chartSettings.domain}
            />
            <Tooltip
              formatter={value => [
                formatValue(value, chartSettings.unit),
                chartSettings.yAxisLabel,
              ]}
              labelFormatter={timestamp => formatTimestamp(timestamp)}
              contentStyle={{
                backgroundColor: theme === 'dark' ? '#333' : '#fff',
                border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
                color: theme === 'dark' ? '#fff' : '#333',
              }}
            />
            <Legend />
            <Area
              type="monotone"
              dataKey={chartSettings.dataKey}
              stroke={chartSettings.color}
              fill={chartSettings.color}
              fillOpacity={0.3}
              activeDot={{ onClick: handleDataPointClick, r: 8 }}
              name={chartSettings.yAxisLabel}
            />
          </AreaChart>
        );

      case 'bar':
        return (
          <BarChart
            data={data}
            margin={{ top: 5, right: 30, left: 20, bottom: 5 }}
          >
            <CartesianGrid
              strokeDasharray="3 3"
              stroke={theme === 'dark' ? '#444' : '#eee'}
            />
            <XAxis
              dataKey="timestamp"
              tickFormatter={formatTimestamp}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
            />
            <YAxis
              label={{
                value: chartSettings.yAxisLabel,
                angle: -90,
                position: 'insideLeft',
                fill: theme === 'dark' ? '#ddd' : '#666',
              }}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
              domain={chartSettings.domain}
            />
            <Tooltip
              formatter={value => [
                formatValue(value, chartSettings.unit),
                chartSettings.yAxisLabel,
              ]}
              labelFormatter={timestamp => formatTimestamp(timestamp)}
              contentStyle={{
                backgroundColor: theme === 'dark' ? '#333' : '#fff',
                border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
                color: theme === 'dark' ? '#fff' : '#333',
              }}
            />
            <Legend />
            <Bar
              dataKey={chartSettings.dataKey}
              fill={chartSettings.color}
              onClick={handleDataPointClick}
              name={chartSettings.yAxisLabel}
            />
          </BarChart>
        );

      case 'composed':
        return (
          <ComposedChart
            data={data}
            margin={{ top: 5, right: 30, left: 20, bottom: 5 }}
          >
            <CartesianGrid
              strokeDasharray="3 3"
              stroke={theme === 'dark' ? '#444' : '#eee'}
            />
            <XAxis
              dataKey="timestamp"
              tickFormatter={formatTimestamp}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
            />
            <YAxis
              label={{
                value: chartSettings.yAxisLabel,
                angle: -90,
                position: 'insideLeft',
                fill: theme === 'dark' ? '#ddd' : '#666',
              }}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
              domain={chartSettings.domain}
            />
            <Tooltip
              formatter={value => [
                formatValue(value, chartSettings.unit),
                chartSettings.yAxisLabel,
              ]}
              labelFormatter={timestamp => formatTimestamp(timestamp)}
              contentStyle={{
                backgroundColor: theme === 'dark' ? '#333' : '#fff',
                border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
                color: theme === 'dark' ? '#fff' : '#333',
              }}
            />
            <Legend />
            <Bar
              dataKey={chartSettings.dataKey}
              fill={chartSettings.color}
              fillOpacity={0.6}
              onClick={handleDataPointClick}
              name={chartSettings.yAxisLabel}
            />
            <Line
              type="monotone"
              dataKey={`${chartSettings.dataKey}Trend`}
              stroke={chartSettings.color}
              dot={false}
              name={`${chartSettings.yAxisLabel} Trend`}
            />
          </ComposedChart>
        );

      default:
        return (
          <LineChart
            data={data}
            margin={{ top: 5, right: 30, left: 20, bottom: 5 }}
          >
            <CartesianGrid
              strokeDasharray="3 3"
              stroke={theme === 'dark' ? '#444' : '#eee'}
            />
            <XAxis
              dataKey="timestamp"
              tickFormatter={formatTimestamp}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
            />
            <YAxis
              label={{
                value: chartSettings.yAxisLabel,
                angle: -90,
                position: 'insideLeft',
                fill: theme === 'dark' ? '#ddd' : '#666',
              }}
              tick={{ fill: theme === 'dark' ? '#ddd' : '#666' }}
              domain={chartSettings.domain}
            />
            <Tooltip
              formatter={value => [
                formatValue(value, chartSettings.unit),
                chartSettings.yAxisLabel,
              ]}
              labelFormatter={timestamp => formatTimestamp(timestamp)}
              contentStyle={{
                backgroundColor: theme === 'dark' ? '#333' : '#fff',
                border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
                color: theme === 'dark' ? '#fff' : '#333',
              }}
            />
            <Legend />
            <Line
              type="monotone"
              dataKey={chartSettings.dataKey}
              stroke={chartSettings.color}
              activeDot={{ onClick: handleDataPointClick, r: 8 }}
              name={chartSettings.yAxisLabel}
            />
          </LineChart>
        );
    }
  };

  // Generate CSS for the component
  const styles = {
    container: {
      backgroundColor: theme === 'dark' ? '#333' : '#fff',
      color: theme === 'dark' ? '#fff' : '#333',
      border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
      borderRadius: '8px',
      padding: '16px',
      maxWidth: '100%',
    },
    header: {
      marginBottom: '16px',
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'flex-start',
    },
    titleContainer: {
      display: 'flex',
      flexDirection: 'column',
    },
    title: {
      fontSize: '18px',
      fontWeight: '600',
      marginBottom: '4px',
    },
    subtitle: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    networkInfo: {
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'flex-end',
      gap: '4px',
    },
    networkInfoItem: {
      fontSize: '13px',
      color: theme === 'dark' ? '#ddd' : '#555',
    },
    highlightedValue: {
      fontWeight: '600',
      color: theme === 'dark' ? '#fff' : '#333',
    },
    chartContainer: {
      height: `${height}px`,
      width: '100%',
    },
    noData: {
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      height: `${height}px`,
      backgroundColor: theme === 'dark' ? '#222' : '#f5f5f5',
      borderRadius: '4px',
      color: theme === 'dark' ? '#aaa' : '#777',
      fontSize: '14px',
    },
    loading: {
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      height: `${height}px`,
      backgroundColor: theme === 'dark' ? '#222' : '#f5f5f5',
      borderRadius: '4px',
      color: theme === 'dark' ? '#aaa' : '#777',
      fontSize: '14px',
    },
    error: {
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      height: `${height}px`,
      backgroundColor: theme === 'dark' ? '#222' : '#f5f5f5',
      borderRadius: '4px',
      color: theme === 'dark' ? '#ff8a8a' : '#cc0000',
      fontSize: '14px',
      padding: '16px',
      textAlign: 'center',
    },
    dataInfo: {
      display: 'flex',
      justifyContent: 'space-between',
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
      marginTop: '8px',
    },
  };

  // Get current metric value from network info
  const getCurrentMetricValue = () => {
    if (!networkInfo) return null;

    switch (metric) {
      case 'blockHeight':
        return networkInfo.blockHeight;
      case 'hashrate':
        return networkInfo.hashrate;
      case 'difficulty':
        return networkInfo.difficulty;
      case 'transactions':
        return networkInfo.transactionsPerBlock;
      case 'fees':
        return networkInfo.averageFee;
      case 'blockTime':
        return networkInfo.blockTime;
      case 'blockSize':
        return networkInfo.blockSize;
      case 'activeNodes':
        return networkInfo.activeNodes;
      default:
        return null;
    }
  };

  return (
    <div style={styles.container} className="blockchain-metrics-chart">
      <div style={styles.header}>
        <div style={styles.titleContainer}>
          <div style={styles.title}>{getChartSettings(metric).yAxisLabel}</div>
          <div style={styles.subtitle}>
            {aggregation === 'minute'
              ? 'By minute'
              : aggregation === 'hourly'
                ? 'Hourly'
                : aggregation === 'daily'
                  ? 'Daily'
                  : aggregation === 'weekly'
                    ? 'Weekly'
                    : 'Monthly'}{' '}
            view
          </div>
        </div>

        {networkInfo && Object.keys(networkInfo).length > 0 && (
          <div style={styles.networkInfo}>
            <div style={styles.networkInfoItem}>
              Network:{' '}
              <span style={styles.highlightedValue}>
                {networkInfo.network || 'Unknown'}
              </span>
            </div>
            {getCurrentMetricValue() !== null && (
              <div style={styles.networkInfoItem}>
                Current:{' '}
                <span style={styles.highlightedValue}>
                  {formatValue(
                    getCurrentMetricValue(),
                    getChartSettings(metric).unit
                  )}
                </span>
              </div>
            )}
          </div>
        )}
      </div>

      <div style={styles.chartContainer}>
        {loading ? (
          <div style={styles.loading}>Loading blockchain metrics...</div>
        ) : error ? (
          <div style={styles.error}>{error}</div>
        ) : data.length === 0 ? (
          <div style={styles.noData}>
            No data available for the selected parameters
          </div>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            {getChartComponent()}
          </ResponsiveContainer>
        )}
      </div>

      {data.length > 0 && !loading && !error && (
        <div style={styles.dataInfo}>
          <div>{`Aggregation: ${aggregation}`}</div>
          <div>{`${data.length} data points`}</div>
        </div>
      )}
    </div>
  );
};

BlockchainMetricsChart.propTypes = {
  metric: PropTypes.oneOf([
    'blockHeight',
    'hashrate',
    'difficulty',
    'transactions',
    'fees',
    'blockTime',
    'blockSize',
    'activeNodes',
  ]),
  timeRange: PropTypes.oneOf([
    '1h',
    '6h',
    '12h',
    '24h',
    '7d',
    '30d',
    '90d',
    '1y',
  ]),
  chartType: PropTypes.oneOf(['line', 'area', 'bar', 'composed']),
  height: PropTypes.number,
  theme: PropTypes.oneOf(['light', 'dark']),
  aggregation: PropTypes.oneOf([
    'minute',
    'hourly',
    'daily',
    'weekly',
    'monthly',
  ]),
  showEvents: PropTypes.bool,
  customColors: PropTypes.object,
  onDataPointClick: PropTypes.func,
  refreshInterval: PropTypes.number,
};

export default BlockchainMetricsChart;
