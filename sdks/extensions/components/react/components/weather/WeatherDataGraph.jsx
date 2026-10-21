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
  Area,
  AreaChart,
  ComposedChart,
  Bar,
} from 'recharts';

/**
 * WeatherDataGraph component for visualizing weather data
 *
 * This component displays historical weather data from Wrfcoin weather stations
 * in various visualization formats with customizable parameters.
 */
const WeatherDataGraph = ({
  stationId,
  dataType = 'temperature',
  timeRange = '24h',
  chartType = 'line',
  showRange = true,
  height = 300,
  theme = 'light',
  aggregation = 'hourly',
  customColors,
  onDataPointClick,
}) => {
  const { sdk } = useWrfcoinContext();
  const [data, setData] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [dataInfo, setDataInfo] = useState(null);

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

  // Get chart settings based on data type
  const getChartSettings = type => {
    const baseSettings = {
      yAxisLabel: '',
      dataKey: 'value',
      unit: '',
      color: '#3a8eff',
      rangeKeys: ['min', 'max'],
      rangeColors: ['#8ed1fc', '#3a8eff'],
      domain: [0, 'auto'],
    };

    switch (type) {
      case 'temperature':
        return {
          ...baseSettings,
          yAxisLabel: 'Temperature',
          unit: '°C',
          color: customColors?.temperature || '#ff9800',
          rangeColors: customColors?.temperatureRange || ['#ffcc80', '#ff9800'],
          domain: ['auto', 'auto'],
        };
      case 'precipitation':
        return {
          ...baseSettings,
          yAxisLabel: 'Precipitation',
          unit: 'mm',
          color: customColors?.precipitation || '#2196f3',
          rangeColors: customColors?.precipitationRange || [
            '#bbdefb',
            '#2196f3',
          ],
          domain: [0, 'auto'],
        };
      case 'humidity':
        return {
          ...baseSettings,
          yAxisLabel: 'Humidity',
          unit: '%',
          color: customColors?.humidity || '#03a9f4',
          rangeColors: customColors?.humidityRange || ['#b3e5fc', '#03a9f4'],
          domain: [0, 100],
        };
      case 'pressure':
        return {
          ...baseSettings,
          yAxisLabel: 'Pressure',
          unit: 'hPa',
          color: customColors?.pressure || '#673ab7',
          rangeColors: customColors?.pressureRange || ['#d1c4e9', '#673ab7'],
          domain: ['auto', 'auto'],
        };
      case 'windSpeed':
        return {
          ...baseSettings,
          yAxisLabel: 'Wind Speed',
          unit: 'km/h',
          color: customColors?.windSpeed || '#4caf50',
          rangeColors: customColors?.windSpeedRange || ['#c8e6c9', '#4caf50'],
          domain: [0, 'auto'],
        };
      case 'solarRadiation':
        return {
          ...baseSettings,
          yAxisLabel: 'Solar Radiation',
          unit: 'W/m²',
          color: customColors?.solarRadiation || '#ffc107',
          rangeColors: customColors?.solarRadiationRange || [
            '#ffecb3',
            '#ffc107',
          ],
          domain: [0, 'auto'],
        };
      case 'airQuality':
        return {
          ...baseSettings,
          yAxisLabel: 'Air Quality (PM2.5)',
          unit: 'µg/m³',
          color: customColors?.airQuality || '#9c27b0',
          rangeColors: customColors?.airQualityRange || ['#e1bee7', '#9c27b0'],
          domain: [0, 'auto'],
        };
      default:
        return baseSettings;
    }
  };

  // Fetch weather data
  useEffect(() => {
    if (!sdk || !stationId) return;

    const fetchWeatherData = async () => {
      try {
        setLoading(true);
        setError(null);

        const timeframeMs = getTimeframeMs(timeRange);
        const now = new Date();
        const startTime = new Date(now.getTime() - timeframeMs);

        // Fetch weather data
        const weatherData = await sdk.getWeatherData({
          stationId: stationId,
          dataType: dataType,
          startTime: startTime.toISOString(),
          endTime: now.toISOString(),
          aggregation: aggregation,
        });

        if (weatherData && weatherData.data) {
          setData(weatherData.data);
          setDataInfo({
            stationName: weatherData.stationName || 'Unknown Station',
            location: weatherData.location || 'Unknown Location',
            unit: weatherData.unit || getChartSettings(dataType).unit,
            dataCount: weatherData.data.length,
          });
        } else {
          setData([]);
          setError('No data available for the selected parameters');
        }
      } catch (err) {
        console.error('Error fetching weather data:', err);
        setError('Failed to load weather data. Please try again later.');
      } finally {
        setLoading(false);
      }
    };

    fetchWeatherData();
  }, [sdk, stationId, dataType, timeRange, aggregation]);

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

  // Handle data point click
  const handleDataPointClick = (data, index) => {
    if (onDataPointClick && typeof onDataPointClick === 'function') {
      onDataPointClick(data, index);
    }
  };

  // Get chart component based on chart type
  const getChartComponent = () => {
    const chartSettings = getChartSettings(dataType);

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
                `${value} ${chartSettings.unit}`,
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
            {showRange &&
              chartSettings.rangeKeys[0] in data[0] &&
              chartSettings.rangeKeys[1] in data[0] && (
                <>
                  <Line
                    type="monotone"
                    dataKey={chartSettings.rangeKeys[0]}
                    stroke={chartSettings.rangeColors[0]}
                    name={`Min ${chartSettings.yAxisLabel}`}
                    strokeDasharray="3 3"
                    dot={false}
                  />
                  <Line
                    type="monotone"
                    dataKey={chartSettings.rangeKeys[1]}
                    stroke={chartSettings.rangeColors[1]}
                    name={`Max ${chartSettings.yAxisLabel}`}
                    strokeDasharray="3 3"
                    dot={false}
                  />
                </>
              )}
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
                `${value} ${chartSettings.unit}`,
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
            {showRange &&
              chartSettings.rangeKeys[0] in data[0] &&
              chartSettings.rangeKeys[1] in data[0] && (
                <Area
                  type="monotone"
                  dataKey={chartSettings.rangeKeys[0]}
                  strokeDasharray="3 3"
                  stroke={chartSettings.rangeColors[0]}
                  fill={chartSettings.rangeColors[0]}
                  fillOpacity={0.1}
                  name={`Min ${chartSettings.yAxisLabel}`}
                />
              )}
          </AreaChart>
        );

      case 'bar':
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
                `${value} ${chartSettings.unit}`,
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
              fillOpacity={0.8}
              onClick={handleDataPointClick}
              name={chartSettings.yAxisLabel}
            />
            {showRange &&
              chartSettings.rangeKeys[0] in data[0] &&
              chartSettings.rangeKeys[1] in data[0] && (
                <>
                  <Line
                    type="monotone"
                    dataKey={chartSettings.rangeKeys[0]}
                    stroke={chartSettings.rangeColors[0]}
                    name={`Min ${chartSettings.yAxisLabel}`}
                    strokeDasharray="3 3"
                    dot={false}
                  />
                  <Line
                    type="monotone"
                    dataKey={chartSettings.rangeKeys[1]}
                    stroke={chartSettings.rangeColors[1]}
                    name={`Max ${chartSettings.yAxisLabel}`}
                    strokeDasharray="3 3"
                    dot={false}
                  />
                </>
              )}
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
                `${value} ${chartSettings.unit}`,
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

  return (
    <div style={styles.container} className="weather-data-graph">
      <div style={styles.header}>
        <div style={styles.title}>
          {dataInfo
            ? `${dataInfo.stationName}: ${getChartSettings(dataType).yAxisLabel}`
            : getChartSettings(dataType).yAxisLabel}
        </div>
        {dataInfo && <div style={styles.subtitle}>{dataInfo.location}</div>}
      </div>

      <div style={styles.chartContainer}>
        {loading ? (
          <div style={styles.loading}>Loading weather data...</div>
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

      {dataInfo && !loading && !error && data.length > 0 && (
        <div style={styles.dataInfo}>
          <div>{`Aggregation: ${aggregation}`}</div>
          <div>{`${dataInfo.dataCount} data points`}</div>
        </div>
      )}
    </div>
  );
};

WeatherDataGraph.propTypes = {
  stationId: PropTypes.string.isRequired,
  dataType: PropTypes.oneOf([
    'temperature',
    'precipitation',
    'humidity',
    'pressure',
    'windSpeed',
    'solarRadiation',
    'airQuality',
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
  chartType: PropTypes.oneOf(['line', 'area', 'bar']),
  showRange: PropTypes.bool,
  height: PropTypes.number,
  theme: PropTypes.oneOf(['light', 'dark']),
  aggregation: PropTypes.oneOf([
    'minute',
    'hourly',
    'daily',
    'weekly',
    'monthly',
  ]),
  customColors: PropTypes.object,
  onDataPointClick: PropTypes.func,
};

export default WeatherDataGraph;
