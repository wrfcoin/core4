import React, { useState, useEffect, useCallback } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * WeatherStationConnect component for connecting to weather stations and IoT devices
 *
 * This component provides a UI for discovering, connecting to, and managing
 * weather stations and IoT devices on the Wrfcoin network.
 */
const WeatherStationConnect = ({
  onConnect,
  onDisconnect,
  onData,
  autoConnect = false,
  deviceFilter,
  theme = 'light',
  className = '',
  supportedDeviceTypes = [
    'weather_station',
    'air_quality',
    'temperature',
    'humidity',
    'pressure',
    'wind',
    'precipitation',
  ],
  connectionMethods = ['bluetooth', 'wifi', 'serial', 'mqtt', 'network'],
  scanTimeout = 10000, // Timeout for device scanning in ms
  showStatus = true,
  showDeviceInfo = true,
  customDeviceRenderer,
}) => {
  const { sdk } = useWrfcoinContext();

  const [isScanning, setIsScanning] = useState(false);
  const [devices, setDevices] = useState([]);
  const [selectedDevice, setSelectedDevice] = useState(null);
  const [connectionStatus, setConnectionStatus] = useState('disconnected');
  const [error, setError] = useState(null);
  const [liveData, setLiveData] = useState(null);
  const [deviceInfo, setDeviceInfo] = useState(null);
  const [connectionMethod, setConnectionMethod] = useState(
    connectionMethods[0]
  );

  // Auto-scan for devices on component mount if autoConnect is true
  useEffect(() => {
    if (autoConnect) {
      scanForDevices();
    }

    // Clean up connections when component unmounts
    return () => {
      if (connectionStatus === 'connected' && selectedDevice) {
        disconnectDevice();
      }
    };
  }, [autoConnect]);

  // Handle new data from the device
  useEffect(() => {
    if (liveData && onData) {
      onData(liveData);
    }
  }, [liveData, onData]);

  // Scan for available devices
  const scanForDevices = async () => {
    try {
      setIsScanning(true);
      setError(null);

      let discoveredDevices;

      if (sdk && sdk.weatherStations && sdk.weatherStations.scan) {
        // Use the SDK method to scan for devices
        discoveredDevices = await sdk.weatherStations.scan({
          deviceTypes: supportedDeviceTypes,
          connectionMethod,
          timeout: scanTimeout,
          filter: deviceFilter,
        });
      } else {
        // For demonstration, use simulated devices
        discoveredDevices = generateMockDevices();
      }

      setDevices(discoveredDevices);
      setIsScanning(false);

      // Auto-connect to the first device if autoConnect is true
      if (autoConnect && discoveredDevices.length > 0) {
        selectDevice(discoveredDevices[0]);
      }
    } catch (err) {
      console.error('Error scanning for devices:', err);
      setError(`Failed to scan for devices: ${err.message}`);
      setIsScanning(false);
    }
  };

  // Generate mock devices for demonstration purposes
  const generateMockDevices = () => {
    return [
      {
        id: 'ws-001',
        name: 'Davis Vantage Pro 2',
        type: 'weather_station',
        connectionMethods: ['wifi', 'bluetooth'],
        address: '00:11:22:33:44:55',
        status: 'available',
        location: {
          latitude: 40.7128,
          longitude: -74.006,
          altitude: 10,
          name: 'New York City',
        },
        capabilities: [
          'temperature',
          'humidity',
          'pressure',
          'wind',
          'precipitation',
          'solarRadiation',
          'uvIndex',
        ],
        lastSeen: new Date().toISOString(),
      },
      {
        id: 'ws-002',
        name: 'AcuRite 5-in-1',
        type: 'weather_station',
        connectionMethods: ['wifi'],
        address: '192.168.1.101',
        status: 'available',
        location: {
          latitude: 40.7128,
          longitude: -74.006,
          altitude: 12,
          name: 'New York City',
        },
        capabilities: [
          'temperature',
          'humidity',
          'pressure',
          'wind',
          'precipitation',
        ],
        lastSeen: new Date().toISOString(),
      },
      {
        id: 'aq-001',
        name: 'PurpleAir Air Quality',
        type: 'air_quality',
        connectionMethods: ['wifi'],
        address: '192.168.1.102',
        status: 'available',
        location: {
          latitude: 40.7128,
          longitude: -74.006,
          altitude: 11,
          name: 'New York City',
        },
        capabilities: ['pm25', 'pm10', 'aqi'],
        lastSeen: new Date().toISOString(),
      },
    ];
  };

  // Select a device
  const selectDevice = device => {
    setSelectedDevice(device);

    // If device selection changes, disconnect from currently connected device
    if (connectionStatus === 'connected') {
      disconnectDevice();
    }
  };

  // Connect to the selected device
  const connectDevice = async () => {
    if (!selectedDevice) {
      setError('No device selected');
      return;
    }

    try {
      setConnectionStatus('connecting');
      setError(null);

      let connectionInfo;

      if (sdk && sdk.weatherStations && sdk.weatherStations.connect) {
        // Use the SDK method to connect to the device
        connectionInfo = await sdk.weatherStations.connect(selectedDevice, {
          connectionMethod,
          timeout: 30000, // 30 second timeout for connection
        });
      } else {
        // For demonstration, use simulated connection info
        connectionInfo = {
          deviceId: selectedDevice.id,
          connectionId: `conn-${Math.random().toString(36).substring(2, 9)}`,
          deviceInfo: {
            manufacturer: 'Davis Instruments',
            model: 'Vantage Pro 2',
            firmwareVersion: '3.24',
            serialNumber: 'WS-' + Math.floor(Math.random() * 10000),
            batteryLevel: 92,
            signalStrength: 86,
          },
        };

        // Simulate connection delay
        await new Promise(resolve => setTimeout(resolve, 1500));
      }

      setConnectionStatus('connected');
      setDeviceInfo(connectionInfo.deviceInfo);

      // Start receiving data from the device
      if (sdk && sdk.weatherStations && sdk.weatherStations.startDataStream) {
        // Use the SDK method to start data stream
        sdk.weatherStations.startDataStream(
          connectionInfo.connectionId,
          handleDataReceived
        );
      } else {
        // For demonstration, start a mock data stream
        startMockDataStream();
      }

      // Call the onConnect callback if provided
      if (onConnect) {
        onConnect(selectedDevice, connectionInfo);
      }
    } catch (err) {
      console.error('Error connecting to device:', err);
      setError(`Failed to connect to device: ${err.message}`);
      setConnectionStatus('disconnected');
    }
  };

  // Disconnect from the device
  const disconnectDevice = async () => {
    try {
      setConnectionStatus('disconnecting');

      if (sdk && sdk.weatherStations && sdk.weatherStations.disconnect) {
        // Use the SDK method to disconnect from the device
        await sdk.weatherStations.disconnect(selectedDevice.id);
      } else {
        // For demonstration, just clear the mock data interval
        if (window.mockDataInterval) {
          clearInterval(window.mockDataInterval);
          window.mockDataInterval = null;
        }

        // Simulate disconnection delay
        await new Promise(resolve => setTimeout(resolve, 500));
      }

      setConnectionStatus('disconnected');
      setLiveData(null);

      // Call the onDisconnect callback if provided
      if (onDisconnect) {
        onDisconnect(selectedDevice);
      }
    } catch (err) {
      console.error('Error disconnecting from device:', err);
      setError(`Failed to disconnect from device: ${err.message}`);
    }
  };

  // Handle data received from the device
  const handleDataReceived = useCallback(data => {
    setLiveData(data);
  }, []);

  // Start a mock data stream for demonstration purposes
  const startMockDataStream = () => {
    if (window.mockDataInterval) {
      clearInterval(window.mockDataInterval);
    }

    window.mockDataInterval = setInterval(() => {
      const now = new Date();

      // Generate mock weather data
      const mockData = {
        timestamp: now.toISOString(),
        deviceId: selectedDevice.id,
        readings: {},
      };

      // Generate values based on device capabilities
      if (selectedDevice.capabilities.includes('temperature')) {
        // Temperature between 15-30°C with some variation
        mockData.readings.temperature =
          22 +
          Math.sin((now.getMinutes() / 60) * Math.PI) * 3 +
          (Math.random() * 0.5 - 0.25);
      }

      if (selectedDevice.capabilities.includes('humidity')) {
        // Humidity between 40-80%
        mockData.readings.humidity =
          60 +
          Math.sin((now.getMinutes() / 60) * Math.PI) * 10 +
          (Math.random() * 2 - 1);
      }

      if (selectedDevice.capabilities.includes('pressure')) {
        // Pressure around 1013 hPa with slight variations
        mockData.readings.pressure =
          1013 +
          Math.sin((now.getMinutes() / 60) * Math.PI) * 2 +
          (Math.random() * 0.4 - 0.2);
      }

      if (selectedDevice.capabilities.includes('wind')) {
        // Wind speed between 0-15 km/h
        mockData.readings.windSpeed = Math.max(
          0,
          5 +
            Math.sin((now.getMinutes() / 60) * Math.PI) * 3 +
            (Math.random() * 2 - 1)
        );
        // Wind direction between 0-359 degrees
        mockData.readings.windDirection =
          (now.getMinutes() * 6 + Math.random() * 10) % 360;
      }

      if (selectedDevice.capabilities.includes('precipitation')) {
        // Precipitation with occasional rain
        const rainChance = Math.sin((now.getMinutes() / 60) * Math.PI) > 0.7;
        mockData.readings.precipitation = rainChance ? Math.random() * 0.8 : 0;
      }

      if (selectedDevice.capabilities.includes('solarRadiation')) {
        // Solar radiation following a day/night pattern
        const hour = now.getHours();
        const dayFactor =
          hour >= 6 && hour <= 18 ? Math.sin(((hour - 6) / 12) * Math.PI) : 0;
        mockData.readings.solarRadiation = Math.max(
          0,
          dayFactor * 800 + (Math.random() * 50 - 25)
        );
      }

      if (selectedDevice.capabilities.includes('uvIndex')) {
        // UV index following a day/night pattern (0-11)
        const hour = now.getHours();
        const dayFactor =
          hour >= 6 && hour <= 18 ? Math.sin(((hour - 6) / 12) * Math.PI) : 0;
        mockData.readings.uvIndex = Math.max(
          0,
          Math.min(11, Math.round(dayFactor * 8 + (Math.random() - 0.5)))
        );
      }

      if (selectedDevice.capabilities.includes('pm25')) {
        // PM2.5 levels (0-500)
        mockData.readings.pm25 = Math.max(
          0,
          15 +
            Math.sin((now.getMinutes() / 60) * Math.PI) * 8 +
            Math.random() * 3
        );
      }

      if (selectedDevice.capabilities.includes('pm10')) {
        // PM10 levels (0-500)
        mockData.readings.pm10 = Math.max(
          0,
          25 +
            Math.sin((now.getMinutes() / 60) * Math.PI) * 10 +
            Math.random() * 5
        );
      }

      if (selectedDevice.capabilities.includes('aqi')) {
        // Air Quality Index (0-500)
        mockData.readings.aqi = Math.max(
          0,
          40 +
            Math.sin((now.getMinutes() / 60) * Math.PI) * 20 +
            Math.random() * 5
        );
      }

      // Update the device's battery level and signal strength periodically
      if (deviceInfo) {
        setDeviceInfo(prev => ({
          ...prev,
          batteryLevel: Math.max(
            0,
            Math.min(100, prev.batteryLevel - Math.random() * 0.01)
          ),
          signalStrength: Math.max(
            0,
            Math.min(100, prev.signalStrength + (Math.random() * 0.2 - 0.1))
          ),
        }));
      }

      // Handle the mock data
      handleDataReceived(mockData);
    }, 1000); // Update every 1 second
  };

  // Change the connection method
  const handleConnectionMethodChange = method => {
    setConnectionMethod(method);
  };

  // Render device list
  const renderDeviceList = () => {
    if (devices.length === 0) {
      return (
        <div className="wrfcoin-weather-station-no-devices">
          <p>No devices found. Please scan for devices.</p>
        </div>
      );
    }

    return (
      <div className="wrfcoin-weather-station-device-list">
        {devices.map(device => (
          <div
            key={device.id}
            className={`wrfcoin-weather-station-device-item ${selectedDevice && selectedDevice.id === device.id ? 'selected' : ''}`}
            onClick={() => selectDevice(device)}
          >
            {customDeviceRenderer ? (
              customDeviceRenderer(device)
            ) : (
              <>
                <div className="wrfcoin-weather-station-device-name">
                  {device.name}
                </div>
                <div className="wrfcoin-weather-station-device-type">
                  {device.type}
                </div>
                <div className="wrfcoin-weather-station-device-connection">
                  {device.connectionMethods.join(', ')}
                </div>
                <div className="wrfcoin-weather-station-device-status">
                  {device.status}
                </div>
              </>
            )}
          </div>
        ))}
      </div>
    );
  };

  // Render connection methods
  const renderConnectionMethods = () => {
    return (
      <div className="wrfcoin-weather-station-connection-methods">
        <label>Connection Method:</label>
        <div className="wrfcoin-weather-station-connection-options">
          {connectionMethods.map(method => (
            <button
              key={method}
              className={`wrfcoin-weather-station-connection-option ${connectionMethod === method ? 'active' : ''}`}
              onClick={() => handleConnectionMethodChange(method)}
              disabled={
                connectionStatus === 'connecting' ||
                connectionStatus === 'connected'
              }
            >
              {method}
            </button>
          ))}
        </div>
      </div>
    );
  };

  // Render device info
  const renderDeviceInfo = () => {
    if (!deviceInfo || !showDeviceInfo) {
      return null;
    }

    return (
      <div className="wrfcoin-weather-station-device-info">
        <h4>Device Information</h4>
        <div className="wrfcoin-weather-station-device-info-grid">
          <div className="wrfcoin-weather-station-device-info-item">
            <span>Manufacturer:</span>
            <span>{deviceInfo.manufacturer}</span>
          </div>
          <div className="wrfcoin-weather-station-device-info-item">
            <span>Model:</span>
            <span>{deviceInfo.model}</span>
          </div>
          <div className="wrfcoin-weather-station-device-info-item">
            <span>Firmware:</span>
            <span>{deviceInfo.firmwareVersion}</span>
          </div>
          <div className="wrfcoin-weather-station-device-info-item">
            <span>Serial Number:</span>
            <span>{deviceInfo.serialNumber}</span>
          </div>
          <div className="wrfcoin-weather-station-device-info-item">
            <span>Battery Level:</span>
            <span>{deviceInfo.batteryLevel}%</span>
          </div>
          <div className="wrfcoin-weather-station-device-info-item">
            <span>Signal Strength:</span>
            <span>{deviceInfo.signalStrength}%</span>
          </div>
        </div>
      </div>
    );
  };

  // Render live data
  const renderLiveData = () => {
    if (!liveData || !liveData.readings) {
      return null;
    }

    return (
      <div className="wrfcoin-weather-station-live-data">
        <h4>Live Data</h4>
        <div className="wrfcoin-weather-station-live-data-timestamp">
          Last Updated: {new Date(liveData.timestamp).toLocaleString()}
        </div>
        <div className="wrfcoin-weather-station-live-data-grid">
          {Object.entries(liveData.readings).map(([key, value]) => (
            <div key={key} className="wrfcoin-weather-station-live-data-item">
              <span>{key}:</span>
              <span>
                {value} {getUnitForReading(key)}
              </span>
            </div>
          ))}
        </div>
      </div>
    );
  };

  // Get unit for reading
  const getUnitForReading = readingType => {
    switch (readingType) {
      case 'temperature':
        return '°C';
      case 'humidity':
        return '%';
      case 'pressure':
        return 'hPa';
      case 'windSpeed':
        return 'km/h';
      case 'windDirection':
        return '°';
      case 'precipitation':
        return 'mm';
      case 'solarRadiation':
        return 'W/m²';
      case 'uvIndex':
        return '';
      case 'pm25':
        return 'µg/m³';
      case 'pm10':
        return 'µg/m³';
      case 'aqi':
        return '';
      default:
        return '';
    }
  };

  return (
    <div
      className={`wrfcoin-weather-station-connect ${className}`}
      data-theme={theme}
    >
      <div className="wrfcoin-weather-station-header">
        <h3>Weather Station Connect</h3>

        {showStatus && (
          <div
            className={`wrfcoin-weather-station-status wrfcoin-status-${connectionStatus}`}
          >
            Status: {connectionStatus}
          </div>
        )}
      </div>

      {error && <div className="wrfcoin-weather-station-error">{error}</div>}

      <div className="wrfcoin-weather-station-controls">
        <button
          className="wrfcoin-weather-station-scan-button"
          onClick={scanForDevices}
          disabled={isScanning}
        >
          {isScanning ? 'Scanning...' : 'Scan for Devices'}
        </button>

        {connectionStatus === 'disconnected' ? (
          <button
            className="wrfcoin-weather-station-connect-button"
            onClick={connectDevice}
            disabled={!selectedDevice || isScanning}
          >
            Connect
          </button>
        ) : (
          <button
            className="wrfcoin-weather-station-disconnect-button"
            onClick={disconnectDevice}
            disabled={connectionStatus !== 'connected'}
          >
            Disconnect
          </button>
        )}
      </div>

      {renderConnectionMethods()}

      <div className="wrfcoin-weather-station-content">
        <div className="wrfcoin-weather-station-devices-container">
          <h4>Available Devices</h4>
          {renderDeviceList()}
        </div>

        <div className="wrfcoin-weather-station-data-container">
          {renderDeviceInfo()}
          {renderLiveData()}
        </div>
      </div>
    </div>
  );
};

WeatherStationConnect.propTypes = {
  onConnect: PropTypes.func,
  onDisconnect: PropTypes.func,
  onData: PropTypes.func,
  autoConnect: PropTypes.bool,
  deviceFilter: PropTypes.object,
  theme: PropTypes.oneOf(['light', 'dark', 'system']),
  className: PropTypes.string,
  supportedDeviceTypes: PropTypes.arrayOf(PropTypes.string),
  connectionMethods: PropTypes.arrayOf(PropTypes.string),
  scanTimeout: PropTypes.number,
  showStatus: PropTypes.bool,
  showDeviceInfo: PropTypes.bool,
  customDeviceRenderer: PropTypes.func,
};

export default WeatherStationConnect;
