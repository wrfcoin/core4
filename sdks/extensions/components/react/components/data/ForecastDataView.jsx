import React, { useState } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * ForecastDataView component for displaying weather forecast data
 *
 * This component displays forecast data with options to view different time periods,
 * variables, and visualization options.
 */
const ForecastDataView = ({
  forecast,
  showRegion = true,
  showTimestamps = true,
  showSource = true,
  compact = false,
  theme = 'light',
  initialPeriod = 'daily',
  initialVariable = 'temperature',
  onDataClick,
}) => {
  const { sdk } = useWrfcoinContext();
  const [activePeriod, setActivePeriod] = useState(initialPeriod);
  const [activeVariable, setActiveVariable] = useState(initialVariable);

  // Format temperature based on unit
  const formatTemperature = (temp, unit = 'celsius') => {
    if (unit === 'celsius') {
      return `${temp.toFixed(1)}°C`;
    } else if (unit === 'fahrenheit') {
      return `${((temp * 9) / 5 + 32).toFixed(1)}°F`;
    }
    return `${temp.toFixed(1)}°`;
  };

  // Format precipitation
  const formatPrecipitation = (precip, unit = 'mm') => {
    return `${precip.toFixed(1)} ${unit}`;
  };

  // Format wind speed
  const formatWindSpeed = (speed, unit = 'mps') => {
    if (unit === 'mps') {
      return `${speed.toFixed(1)} m/s`;
    } else if (unit === 'kmh') {
      return `${(speed * 3.6).toFixed(1)} km/h`;
    } else if (unit === 'mph') {
      return `${(speed * 2.237).toFixed(1)} mph`;
    }
    return `${speed.toFixed(1)} ${unit}`;
  };

  // Format timestamp
  const formatTimestamp = (timestamp, format = 'full') => {
    if (!timestamp) return '';

    const date = new Date(timestamp);

    if (format === 'date') {
      return date.toLocaleDateString();
    } else if (format === 'time') {
      return date.toLocaleTimeString([], {
        hour: '2-digit',
        minute: '2-digit',
      });
    } else if (format === 'dayTime') {
      return `${date.toLocaleDateString([], { weekday: 'short' })} ${date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
    } else if (format === 'day') {
      return date.toLocaleDateString([], { weekday: 'short' });
    }

    return date.toLocaleString();
  };

  // Format value based on variable type
  const formatValue = (value, variable) => {
    if (!value) return 'N/A';

    switch (variable) {
      case 'temperature':
        return formatTemperature(
          value,
          forecast.units?.temperature || 'celsius'
        );
      case 'precipitation':
        return formatPrecipitation(
          value,
          forecast.units?.precipitation || 'mm'
        );
      case 'humidity':
        return `${value.toFixed(0)}%`;
      case 'windSpeed':
        return formatWindSpeed(value, forecast.units?.windSpeed || 'mps');
      case 'pressure':
        return `${value.toFixed(1)} ${forecast.units?.pressure || 'hPa'}`;
      case 'cloudCover':
        return `${value.toFixed(0)}%`;
      case 'visibility':
        return `${value.toFixed(1)} km`;
      default:
        return value.toString();
    }
  };

  // Get color for temperature visualization
  const getTemperatureColor = temp => {
    if (temp <= -10) return '#0022cc';
    if (temp <= 0) return '#0066ff';
    if (temp <= 10) return '#00aaff';
    if (temp <= 15) return '#33ccff';
    if (temp <= 20) return '#99ff99';
    if (temp <= 25) return '#ffff00';
    if (temp <= 30) return '#ffcc00';
    if (temp <= 35) return '#ff6600';
    return '#ff0000';
  };

  // Get color for precipitation visualization
  const getPrecipitationColor = precip => {
    if (precip === 0) return '#ffffff';
    if (precip < 0.5) return '#e6f9ff';
    if (precip < 2) return '#ccf2ff';
    if (precip < 5) return '#99e6ff';
    if (precip < 10) return '#66d9ff';
    if (precip < 20) return '#33ccff';
    if (precip < 30) return '#00bfff';
    if (precip < 50) return '#0099ff';
    return '#0066cc';
  };

  // Get color for humidity visualization
  const getHumidityColor = humidity => {
    if (humidity < 30) return '#ffe6e6';
    if (humidity < 50) return '#ffcccc';
    if (humidity < 70) return '#ccccff';
    if (humidity < 90) return '#9999ff';
    return '#6666ff';
  };

  // Get the background color for a value based on variable
  const getValueBackgroundColor = (value, variable) => {
    if (value === undefined || value === null) return 'transparent';

    switch (variable) {
      case 'temperature':
        return getTemperatureColor(value);
      case 'precipitation':
        return getPrecipitationColor(value);
      case 'humidity':
        return getHumidityColor(value);
      default:
        return 'transparent';
    }
  };

  // Get the text color based on background color
  const getTextColorForBackground = bgColor => {
    if (bgColor === 'transparent') return theme === 'dark' ? '#fff' : '#333';

    // Extract the RGB values from the hex color
    const r = parseInt(bgColor.slice(1, 3), 16);
    const g = parseInt(bgColor.slice(3, 5), 16);
    const b = parseInt(bgColor.slice(5, 7), 16);

    // Calculate the perceptive luminance - a weighted average
    const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;

    // Return black for bright colors and white for dark ones
    return luminance > 0.5 ? '#333' : '#fff';
  };

  // Get forecast data for current period and variable
  const getForecastData = () => {
    if (!forecast || !forecast.periods) {
      return [];
    }

    let data = [];

    if (activePeriod === 'hourly' && forecast.periods.hourly) {
      data = forecast.periods.hourly;
    } else if (activePeriod === 'daily' && forecast.periods.daily) {
      data = forecast.periods.daily;
    } else if (activePeriod === '3hourly' && forecast.periods['3hourly']) {
      data = forecast.periods['3hourly'];
    } else if (activePeriod === '6hourly' && forecast.periods['6hourly']) {
      data = forecast.periods['6hourly'];
    }

    return data;
  };

  // Handle data click
  const handleDataClick = periodData => {
    if (onDataClick && typeof onDataClick === 'function') {
      onDataClick(periodData);
    }
  };

  // Get theme-specific class
  const getThemeClass = () => {
    return theme === 'dark'
      ? 'forecast-data-view-dark'
      : 'forecast-data-view-light';
  };

  // Generate CSS for the component
  const styles = {
    container: {
      border: theme === 'dark' ? '1px solid #444' : '1px solid #ddd',
      borderRadius: '8px',
      padding: compact ? '8px' : '16px',
      marginBottom: '16px',
      backgroundColor: theme === 'dark' ? '#333' : '#fff',
      color: theme === 'dark' ? '#fff' : '#333',
      boxShadow: '0 2px 4px rgba(0, 0, 0, 0.1)',
      maxWidth: '100%',
      overflow: 'hidden',
    },
    header: {
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'flex-start',
      marginBottom: '12px',
      flexDirection: compact ? 'column' : 'row',
      gap: compact ? '8px' : '0',
    },
    title: {
      fontSize: compact ? '16px' : '18px',
      fontWeight: 'bold',
      margin: 0,
      marginBottom: compact ? '4px' : '8px',
    },
    region: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#666',
      marginBottom: '4px',
    },
    timestamp: {
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    controls: {
      display: 'flex',
      gap: '8px',
      marginBottom: '16px',
      flexWrap: 'wrap',
    },
    select: {
      padding: '6px 10px',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      borderRadius: '4px',
      backgroundColor: theme === 'dark' ? '#444' : '#f5f5f5',
      color: theme === 'dark' ? '#fff' : '#333',
      fontSize: '14px',
    },
    periodButton: isActive => ({
      padding: '6px 12px',
      border: 'none',
      borderRadius: '4px',
      backgroundColor: isActive
        ? theme === 'dark'
          ? '#3a8eff'
          : '#3a8eff'
        : theme === 'dark'
          ? '#444'
          : '#f0f0f0',
      color: isActive ? '#fff' : theme === 'dark' ? '#ddd' : '#555',
      cursor: 'pointer',
      fontSize: '14px',
      fontWeight: isActive ? '500' : 'normal',
      transition: 'all 0.2s ease',
    }),
    dataGrid: {
      display: 'grid',
      gridTemplateColumns: 'repeat(auto-fill, minmax(100px, 1fr))',
      gap: '8px',
      overflowX: 'auto',
    },
    dataCell: bgColor => ({
      padding: '8px',
      backgroundColor: bgColor,
      borderRadius: '4px',
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      cursor: onDataClick ? 'pointer' : 'default',
      transition: 'transform 0.2s ease',
      '&:hover': {
        transform: 'scale(1.02)',
      },
    }),
    periodLabel: bgColor => ({
      fontWeight: '500',
      fontSize: '14px',
      marginBottom: '4px',
      color: getTextColorForBackground(bgColor),
    }),
    dataValue: bgColor => ({
      fontSize: '16px',
      fontWeight: '600',
      color: getTextColorForBackground(bgColor),
    }),
    footer: {
      marginTop: '12px',
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
      display: 'flex',
      justifyContent: 'space-between',
    },
    source: {
      fontSize: '11px',
      color: theme === 'dark' ? '#999' : '#888',
    },
    noData: {
      padding: '16px',
      textAlign: 'center',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
  };

  // Get available periods from forecast data
  const getAvailablePeriods = () => {
    if (!forecast || !forecast.periods) return [];

    const periods = [];
    if (forecast.periods.hourly && forecast.periods.hourly.length > 0) {
      periods.push('hourly');
    }
    if (forecast.periods['3hourly'] && forecast.periods['3hourly'].length > 0) {
      periods.push('3hourly');
    }
    if (forecast.periods['6hourly'] && forecast.periods['6hourly'].length > 0) {
      periods.push('6hourly');
    }
    if (forecast.periods.daily && forecast.periods.daily.length > 0) {
      periods.push('daily');
    }

    return periods;
  };

  // Get available variables from forecast data
  const getAvailableVariables = () => {
    if (!forecast) return [];

    const variables = [];
    const data = getForecastData();

    if (data.length === 0) return [];

    // Check first period for available variables
    const firstPeriod = data[0];

    if (firstPeriod.temperature !== undefined) variables.push('temperature');
    if (firstPeriod.precipitation !== undefined)
      variables.push('precipitation');
    if (firstPeriod.humidity !== undefined) variables.push('humidity');
    if (firstPeriod.windSpeed !== undefined) variables.push('windSpeed');
    if (firstPeriod.pressure !== undefined) variables.push('pressure');
    if (firstPeriod.cloudCover !== undefined) variables.push('cloudCover');
    if (firstPeriod.visibility !== undefined) variables.push('visibility');

    return variables;
  };

  // Get variable display name
  const getVariableDisplayName = variableName => {
    switch (variableName) {
      case 'temperature':
        return 'Temperature';
      case 'precipitation':
        return 'Precipitation';
      case 'humidity':
        return 'Humidity';
      case 'windSpeed':
        return 'Wind Speed';
      case 'pressure':
        return 'Pressure';
      case 'cloudCover':
        return 'Cloud Cover';
      case 'visibility':
        return 'Visibility';
      default:
        return variableName;
    }
  };

  // Get period display name
  const getPeriodDisplayName = periodName => {
    switch (periodName) {
      case 'hourly':
        return 'Hourly';
      case '3hourly':
        return '3-Hour';
      case '6hourly':
        return '6-Hour';
      case 'daily':
        return 'Daily';
      default:
        return periodName;
    }
  };

  // Get timestamp format for period
  const getTimestampFormatForPeriod = period => {
    switch (period) {
      case 'hourly':
        return 'time';
      case '3hourly':
        return 'dayTime';
      case '6hourly':
        return 'dayTime';
      case 'daily':
        return 'date';
      default:
        return 'full';
    }
  };

  const forecastData = getForecastData();
  const availablePeriods = getAvailablePeriods();
  const availableVariables = getAvailableVariables();

  // If activePeriod is not available, set it to the first available period
  if (availablePeriods.length > 0 && !availablePeriods.includes(activePeriod)) {
    setActivePeriod(availablePeriods[0]);
  }

  // If activeVariable is not available, set it to the first available variable
  if (
    availableVariables.length > 0 &&
    !availableVariables.includes(activeVariable)
  ) {
    setActiveVariable(availableVariables[0]);
  }

  return (
    <div
      style={styles.container}
      className={`forecast-data-view ${getThemeClass()}`}
    >
      <div style={styles.header}>
        <div>
          <h3 style={styles.title}>Weather Forecast</h3>
          {showRegion && forecast.region && (
            <div style={styles.region}>Region: {forecast.region}</div>
          )}
          {showTimestamps && forecast.forecastTimestamp && (
            <div style={styles.timestamp}>
              Generated: {formatTimestamp(forecast.forecastTimestamp)}
            </div>
          )}
        </div>
      </div>

      {forecast && (
        <div style={styles.controls}>
          {availablePeriods.length > 0 && (
            <div>
              {availablePeriods.map(period => (
                <button
                  key={period}
                  style={styles.periodButton(period === activePeriod)}
                  onClick={() => setActivePeriod(period)}
                >
                  {getPeriodDisplayName(period)}
                </button>
              ))}
            </div>
          )}

          {availableVariables.length > 1 && (
            <select
              style={styles.select}
              value={activeVariable}
              onChange={e => setActiveVariable(e.target.value)}
            >
              {availableVariables.map(variable => (
                <option key={variable} value={variable}>
                  {getVariableDisplayName(variable)}
                </option>
              ))}
            </select>
          )}
        </div>
      )}

      {forecastData.length === 0 ? (
        <div style={styles.noData}>No forecast data available</div>
      ) : (
        <div style={styles.dataGrid}>
          {forecastData.map((periodData, index) => {
            const value = periodData[activeVariable];
            const bgColor = getValueBackgroundColor(value, activeVariable);

            return (
              <div
                key={index}
                style={styles.dataCell(bgColor)}
                onClick={() => handleDataClick(periodData)}
              >
                <div style={styles.periodLabel(bgColor)}>
                  {formatTimestamp(
                    periodData.timestamp,
                    getTimestampFormatForPeriod(activePeriod)
                  )}
                </div>
                <div style={styles.dataValue(bgColor)}>
                  {formatValue(value, activeVariable)}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {showSource && forecast.source && (
        <div style={styles.footer}>
          <div style={styles.source}>Source: {forecast.source}</div>
          {forecast.validUntil && (
            <div style={styles.source}>
              Valid until: {formatTimestamp(forecast.validUntil)}
            </div>
          )}
        </div>
      )}
    </div>
  );
};

ForecastDataView.propTypes = {
  forecast: PropTypes.shape({
    id: PropTypes.string,
    region: PropTypes.string,
    forecastTimestamp: PropTypes.string,
    validUntil: PropTypes.string,
    source: PropTypes.string,
    model: PropTypes.string,
    confidence: PropTypes.number,
    units: PropTypes.shape({
      temperature: PropTypes.string,
      precipitation: PropTypes.string,
      windSpeed: PropTypes.string,
      pressure: PropTypes.string,
    }),
    periods: PropTypes.shape({
      hourly: PropTypes.arrayOf(PropTypes.object),
      '3hourly': PropTypes.arrayOf(PropTypes.object),
      '6hourly': PropTypes.arrayOf(PropTypes.object),
      daily: PropTypes.arrayOf(PropTypes.object),
    }),
  }).isRequired,
  showRegion: PropTypes.bool,
  showTimestamps: PropTypes.bool,
  showSource: PropTypes.bool,
  compact: PropTypes.bool,
  theme: PropTypes.oneOf(['light', 'dark']),
  initialPeriod: PropTypes.oneOf(['hourly', '3hourly', '6hourly', 'daily']),
  initialVariable: PropTypes.string,
  onDataClick: PropTypes.func,
};

export default ForecastDataView;
