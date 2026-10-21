import React, { useState, useEffect, useRef } from 'react';
import PropTypes from 'prop-types';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from 'recharts';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * WeatherDataChart component for visualizing weather data
 *
 * This component renders a line chart for weather data visualization
 * with support for multiple metrics and customizable appearance.
 */
const WeatherDataChart = ({
  location,
  timeframe,
  metrics = ['temperature'],
  height = 400,
  width = '100%',
  theme = 'light',
  title,
  showLegend = true,
  showGrid = true,
  colors = {
    temperature: '#FF5722',
    precipitation: '#2196F3',
    humidity: '#4CAF50',
    pressure: '#9C27B0',
    windSpeed: '#FF9800',
    solarRadiation: '#FFEB3B',
    uvIndex: '#F44336',
  },
  onDataPointClick,
  customTooltip,
  className = '',
  responsive = {},
}) => {
  const { sdk } = useWrfcoinContext();
  const [data, setData] = useState([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState(null);
  const chartRef = useRef(null);

  // Fetch weather data on component mount and when dependencies change
  useEffect(() => {
    if (location) {
      fetchWeatherData();
    }
  }, [location, timeframe, metrics, sdk]);

  // Fetch weather data from API
  const fetchWeatherData = async () => {
    try {
      setIsLoading(true);
      setError(null);

      // Use the SDK to fetch weather data
      let weatherData;

      if (sdk && sdk.getWeatherData) {
        weatherData = await sdk.getWeatherData({
          location,
          timeframe,
          metrics,
        });
      } else {
        // If SDK is not available, use mock data for demonstration
        weatherData = generateMockWeatherData();
      }

      // Process and format the data for the chart
      const formattedData = formatWeatherData(weatherData);

      setData(formattedData);
      setIsLoading(false);
    } catch (err) {
      console.error('Error fetching weather data:', err);
      setError('Failed to fetch weather data. Please try again later.');
      setIsLoading(false);
    }
  };

  // Format weather data for the chart
  const formatWeatherData = weatherData => {
    if (!weatherData || !weatherData.data) {
      return [];
    }

    return weatherData.data.map(item => {
      const formattedItem = {
        timestamp: new Date(item.timestamp).toLocaleString(),
        date: new Date(item.timestamp),
      };

      // Add metrics to the formatted item
      metrics.forEach(metric => {
        if (item[metric] !== undefined) {
          formattedItem[metric] = item[metric];
        }
      });

      return formattedItem;
    });
  };

  // Generate mock weather data for demonstration
  const generateMockWeatherData = () => {
    const now = new Date();
    const data = [];

    // Generate data points for the past 7 days
    for (let i = 0; i < 24 * 7; i++) {
      const timestamp = new Date(now);
      timestamp.setHours(now.getHours() - i);

      const dataPoint = {
        timestamp: timestamp.toISOString(),
      };

      // Generate mock values for requested metrics
      if (metrics.includes('temperature')) {
        // Temperature between 15-30°C with some variation
        dataPoint.temperature =
          22 + Math.sin(i / 12) * 7 + (Math.random() * 2 - 1);
      }

      if (metrics.includes('precipitation')) {
        // Precipitation between 0-10mm with occasional spikes
        dataPoint.precipitation = Math.max(
          0,
          Math.sin(i / 8) * 3 + Math.random() * 2
        );
      }

      if (metrics.includes('humidity')) {
        // Humidity between 40-90%
        dataPoint.humidity =
          65 + Math.sin(i / 10) * 15 + (Math.random() * 10 - 5);
      }

      if (metrics.includes('pressure')) {
        // Pressure around 1013 hPa with slight variations
        dataPoint.pressure =
          1013 + Math.sin(i / 24) * 5 + (Math.random() * 2 - 1);
      }

      if (metrics.includes('windSpeed')) {
        // Wind speed between 0-20 km/h
        dataPoint.windSpeed = Math.max(
          0,
          Math.sin(i / 6) * 8 + Math.random() * 4
        );
      }

      if (metrics.includes('solarRadiation')) {
        // Solar radiation following a day/night pattern
        const hour = timestamp.getHours();
        const dayFactor =
          hour >= 6 && hour <= 18 ? Math.sin(((hour - 6) / 12) * Math.PI) : 0;
        dataPoint.solarRadiation = Math.max(
          0,
          dayFactor * 1000 + (Math.random() * 100 - 50)
        );
      }

      if (metrics.includes('uvIndex')) {
        // UV index following a day/night pattern (0-11)
        const hour = timestamp.getHours();
        const dayFactor =
          hour >= 6 && hour <= 18 ? Math.sin(((hour - 6) / 12) * Math.PI) : 0;
        dataPoint.uvIndex = Math.max(
          0,
          Math.min(11, Math.round(dayFactor * 10 + (Math.random() * 2 - 1)))
        );
      }

      data.push(dataPoint);
    }

    return { data };
  };

  // Handle data point click
  const handleDataPointClick = dataPoint => {
    if (onDataPointClick) {
      onDataPointClick(dataPoint);
    }
  };

  // Generate units for each metric
  const getMetricUnit = metric => {
    switch (metric) {
      case 'temperature':
        return '°C';
      case 'precipitation':
        return 'mm';
      case 'humidity':
        return '%';
      case 'pressure':
        return 'hPa';
      case 'windSpeed':
        return 'km/h';
      case 'solarRadiation':
        return 'W/m²';
      case 'uvIndex':
        return '';
      default:
        return '';
    }
  };

  // Generate y-axis label based on metrics
  const getYAxisLabel = () => {
    if (metrics.length === 1) {
      return `${metrics[0]} (${getMetricUnit(metrics[0])})`;
    }
    return 'Value';
  };

  // Custom tooltip component
  const renderCustomTooltip = props => {
    if (customTooltip) {
      return customTooltip(props);
    }

    const { active, payload, label } = props;

    if (active && payload && payload.length) {
      return (
        <div
          className="wrfcoin-chart-tooltip"
          style={{
            backgroundColor: theme === 'dark' ? '#333' : '#fff',
            padding: '10px',
            border: '1px solid #ccc',
          }}
        >
          <p className="wrfcoin-chart-tooltip-label">{label}</p>
          {payload.map((entry, index) => (
            <p key={`tooltip-${index}`} style={{ color: entry.stroke }}>
              {`${entry.name}: ${entry.value.toFixed(2)} ${getMetricUnit(entry.name)}`}
            </p>
          ))}
        </div>
      );
    }

    return null;
  };

  // Apply responsive configuration based on screen size
  const applyResponsiveConfig = () => {
    if (!responsive || Object.keys(responsive).length === 0) {
      return { height, showLegend };
    }

    // Determine current screen size
    const getScreenSize = () => {
      const width = window.innerWidth;
      if (width < 576) return 'mobile';
      if (width < 992) return 'tablet';
      return 'desktop';
    };

    const screenSize = getScreenSize();
    const config = responsive[screenSize] || {};

    return {
      height: config.height || height,
      showLegend:
        config.showLegend !== undefined ? config.showLegend : showLegend,
      simplifiedView: config.simplifiedView || false,
    };
  };

  const responsiveConfig = applyResponsiveConfig();

  // Loading and error states
  if (isLoading) {
    return (
      <div
        className={`wrfcoin-weather-chart-loading ${className}`}
        data-theme={theme}
      >
        <p>Loading weather data...</p>
      </div>
    );
  }

  if (error) {
    return (
      <div
        className={`wrfcoin-weather-chart-error ${className}`}
        data-theme={theme}
      >
        <p>{error}</p>
        <button onClick={fetchWeatherData}>Retry</button>
      </div>
    );
  }

  return (
    <div
      className={`wrfcoin-weather-chart ${className}`}
      data-theme={theme}
      ref={chartRef}
      style={{ height: responsiveConfig.height, width }}
    >
      {title && <h3 className="wrfcoin-weather-chart-title">{title}</h3>}

      <ResponsiveContainer width="100%" height="100%">
        <LineChart
          data={data}
          margin={{ top: 20, right: 30, left: 20, bottom: 20 }}
          onClick={handleDataPointClick}
        >
          {showGrid && <CartesianGrid strokeDasharray="3 3" />}

          <XAxis
            dataKey="timestamp"
            tickFormatter={value => {
              const date = new Date(value);
              return responsiveConfig.simplifiedView
                ? `${date.getHours()}:00`
                : `${date.getMonth() + 1}/${date.getDate()} ${date.getHours()}:00`;
            }}
          />

          <YAxis
            label={{
              value: getYAxisLabel(),
              angle: -90,
              position: 'insideLeft',
            }}
          />

          <Tooltip content={renderCustomTooltip} />

          {responsiveConfig.showLegend && <Legend />}

          {metrics.map(metric => (
            <Line
              key={metric}
              type="monotone"
              dataKey={metric}
              stroke={colors[metric] || '#000'}
              activeDot={{ r: 8, onClick: handleDataPointClick }}
              dot={false}
            />
          ))}
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
};

WeatherDataChart.propTypes = {
  location: PropTypes.shape({
    lat: PropTypes.number.isRequired,
    lng: PropTypes.number.isRequired,
    name: PropTypes.string,
  }).isRequired,
  timeframe: PropTypes.shape({
    start: PropTypes.string,
    end: PropTypes.string,
  }),
  metrics: PropTypes.arrayOf(PropTypes.string),
  height: PropTypes.oneOfType([PropTypes.number, PropTypes.string]),
  width: PropTypes.oneOfType([PropTypes.number, PropTypes.string]),
  theme: PropTypes.oneOf(['light', 'dark', 'system']),
  title: PropTypes.string,
  showLegend: PropTypes.bool,
  showGrid: PropTypes.bool,
  colors: PropTypes.object,
  onDataPointClick: PropTypes.func,
  customTooltip: PropTypes.func,
  className: PropTypes.string,
  responsive: PropTypes.object,
};

export default WeatherDataChart;
