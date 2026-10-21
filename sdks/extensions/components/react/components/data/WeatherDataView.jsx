import React from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * WeatherDataView component for displaying weather data from stations
 *
 * This component displays weather data from stations in a readable format,
 * handling different data types and formatting values appropriately.
 */
const WeatherDataView = ({
  data,
  stationInfo,
  showTimestamp = true,
  showLocation = true,
  compact = false,
  theme = 'light',
  onDataClick,
}) => {
  const { sdk } = useWrfcoinContext();

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

  // Format pressure
  const formatPressure = (pressure, unit = 'hPa') => {
    return `${pressure.toFixed(1)} ${unit}`;
  };

  // Format humidity
  const formatHumidity = humidity => {
    return `${humidity.toFixed(0)}%`;
  };

  // Format timestamp
  const formatTimestamp = timestamp => {
    const date = new Date(timestamp);
    return date.toLocaleString();
  };

  // Format location
  const formatLocation = (lat, lng) => {
    return `${lat.toFixed(4)}, ${lng.toFixed(4)}`;
  };

  // Handle clicking on data
  const handleDataClick = () => {
    if (onDataClick && typeof onDataClick === 'function') {
      onDataClick(data);
    }
  };

  // Get station name
  const getStationName = () => {
    if (stationInfo && stationInfo.name) {
      return stationInfo.name;
    } else if (data.stationId) {
      return `Station ${data.stationId}`;
    }
    return 'Unknown Station';
  };

  // Get verification status icon
  const getVerificationIcon = () => {
    if (data.verified) {
      return '✓'; // Verified
    } else if (data.verification && data.verification.status === 'pending') {
      return '⋯'; // Pending
    }
    return '?'; // Unknown
  };

  // Get theme-specific class
  const getThemeClass = () => {
    return theme === 'dark'
      ? 'weather-data-view-dark'
      : 'weather-data-view-light';
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
      cursor: onDataClick ? 'pointer' : 'default',
      transition: 'transform 0.2s ease',
      maxWidth: '100%',
      overflow: 'hidden',
    },
    header: {
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      marginBottom: '8px',
    },
    stationName: {
      fontSize: compact ? '14px' : '16px',
      fontWeight: 'bold',
      marginRight: '8px',
    },
    timestamp: {
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    location: {
      fontSize: '12px',
      marginBottom: '8px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    dataGrid: {
      display: 'grid',
      gridTemplateColumns: compact ? '1fr 1fr' : '1fr 1fr 1fr',
      gap: '8px',
    },
    dataItem: {
      marginBottom: '4px',
    },
    label: {
      color: theme === 'dark' ? '#aaa' : '#777',
      fontSize: '12px',
      marginBottom: '2px',
    },
    value: {
      fontSize: compact ? '14px' : '16px',
      fontWeight: '500',
    },
    footer: {
      marginTop: '8px',
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
      display: 'flex',
      justifyContent: 'space-between',
    },
    verificationBadge: {
      display: 'inline-flex',
      alignItems: 'center',
      padding: '2px 6px',
      borderRadius: '4px',
      fontSize: '11px',
      backgroundColor: data.verified
        ? theme === 'dark'
          ? '#1c4c1c'
          : '#e6ffe6'
        : theme === 'dark'
          ? '#4c3c1c'
          : '#fff6e6',
      color: data.verified
        ? theme === 'dark'
          ? '#8aff8a'
          : '#008000'
        : theme === 'dark'
          ? '#ffcc8a'
          : '#cc8800',
      marginLeft: '8px',
    },
  };

  return (
    <div
      style={styles.container}
      className={`weather-data-view ${getThemeClass()}`}
      onClick={handleDataClick}
    >
      <div style={styles.header}>
        <div style={styles.stationName}>
          {getStationName()}
          <span style={styles.verificationBadge}>
            {getVerificationIcon()} {data.verified ? 'Verified' : 'Pending'}
          </span>
        </div>
        {showTimestamp && data.timestamp && (
          <div style={styles.timestamp}>{formatTimestamp(data.timestamp)}</div>
        )}
      </div>

      {showLocation && data.location && (
        <div style={styles.location}>
          Location:{' '}
          {formatLocation(data.location.latitude, data.location.longitude)}
        </div>
      )}

      <div style={styles.dataGrid}>
        {data.temperature !== undefined && (
          <div style={styles.dataItem}>
            <div style={styles.label}>Temperature</div>
            <div style={styles.value}>
              {formatTemperature(
                data.temperature,
                data.units?.temperature || 'celsius'
              )}
            </div>
          </div>
        )}

        {data.precipitation !== undefined && (
          <div style={styles.dataItem}>
            <div style={styles.label}>Precipitation</div>
            <div style={styles.value}>
              {formatPrecipitation(
                data.precipitation,
                data.units?.precipitation || 'mm'
              )}
            </div>
          </div>
        )}

        {data.windSpeed !== undefined && (
          <div style={styles.dataItem}>
            <div style={styles.label}>Wind Speed</div>
            <div style={styles.value}>
              {formatWindSpeed(data.windSpeed, data.units?.windSpeed || 'mps')}
            </div>
          </div>
        )}

        {data.pressure !== undefined && (
          <div style={styles.dataItem}>
            <div style={styles.label}>Pressure</div>
            <div style={styles.value}>
              {formatPressure(data.pressure, data.units?.pressure || 'hPa')}
            </div>
          </div>
        )}

        {data.humidity !== undefined && (
          <div style={styles.dataItem}>
            <div style={styles.label}>Humidity</div>
            <div style={styles.value}>{formatHumidity(data.humidity)}</div>
          </div>
        )}

        {data.windDirection !== undefined && (
          <div style={styles.dataItem}>
            <div style={styles.label}>Wind Direction</div>
            <div style={styles.value}>{data.windDirection}°</div>
          </div>
        )}
      </div>

      <div style={styles.footer}>
        <div>
          {data.blockchainRef &&
            `Blockchain Ref: ${data.blockchainRef.slice(0, 8)}...`}
        </div>
        <div>{data.sensorType && `Sensor Type: ${data.sensorType}`}</div>
      </div>
    </div>
  );
};

WeatherDataView.propTypes = {
  data: PropTypes.shape({
    id: PropTypes.string,
    stationId: PropTypes.string,
    timestamp: PropTypes.string,
    temperature: PropTypes.number,
    precipitation: PropTypes.number,
    windSpeed: PropTypes.number,
    windDirection: PropTypes.number,
    pressure: PropTypes.number,
    humidity: PropTypes.number,
    location: PropTypes.shape({
      latitude: PropTypes.number,
      longitude: PropTypes.number,
      altitude: PropTypes.number,
    }),
    units: PropTypes.shape({
      temperature: PropTypes.string,
      precipitation: PropTypes.string,
      windSpeed: PropTypes.string,
      pressure: PropTypes.string,
    }),
    verified: PropTypes.bool,
    verification: PropTypes.shape({
      status: PropTypes.string,
      verifiedBy: PropTypes.string,
      timestamp: PropTypes.string,
    }),
    blockchainRef: PropTypes.string,
    sensorType: PropTypes.string,
  }).isRequired,
  stationInfo: PropTypes.shape({
    id: PropTypes.string,
    name: PropTypes.string,
    location: PropTypes.shape({
      latitude: PropTypes.number,
      longitude: PropTypes.number,
      altitude: PropTypes.number,
    }),
    type: PropTypes.string,
  }),
  showTimestamp: PropTypes.bool,
  showLocation: PropTypes.bool,
  compact: PropTypes.bool,
  theme: PropTypes.oneOf(['light', 'dark']),
  onDataClick: PropTypes.func,
};

export default WeatherDataView;
