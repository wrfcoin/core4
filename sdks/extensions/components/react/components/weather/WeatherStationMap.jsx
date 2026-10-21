import React, { useState, useEffect, useRef } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * WeatherStationMap component for displaying weather stations on a map
 *
 * This component visualizes weather stations on an interactive map with
 * filtering and selection capabilities. It uses Leaflet for the map display.
 *
 * Note: This component requires the leaflet library to be installed:
 * npm install leaflet
 */
const WeatherStationMap = ({
  onStationSelect,
  region = null,
  dataTypes = null,
  theme = 'light',
  initialSelected = null,
  mapCenter = [0, 0],
  mapZoom = 3,
  height = 400,
  showPopup = true,
  showControls = true,
  clusterMarkers = true,
}) => {
  const { sdk } = useWrfcoinContext();
  const [stations, setStations] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [selectedStation, setSelectedStation] = useState(initialSelected);
  const [map, setMap] = useState(null);
  const [markers, setMarkers] = useState([]);
  const [userLocation, setUserLocation] = useState(null);
  const [filter, setFilter] = useState('');
  const [filteredStations, setFilteredStations] = useState([]);
  const mapContainerRef = useRef(null);
  const leafletRef = useRef(null);
  const markerClusterRef = useRef(null);

  // Fetch leaflet if not available
  useEffect(() => {
    if (typeof window !== 'undefined' && !leafletRef.current) {
      import('leaflet')
        .then(L => {
          leafletRef.current = L;

          // Additional Leaflet dependencies
          if (clusterMarkers) {
            import('leaflet.markercluster').catch(err => {
              console.warn('Failed to load leaflet.markercluster:', err);
            });
          }

          initializeMap();
        })
        .catch(err => {
          console.error('Failed to load leaflet:', err);
          setError(
            'Failed to load map library. Please ensure leaflet is installed.'
          );
        });
    }
  }, []);

  // Initialize map
  const initializeMap = () => {
    if (!leafletRef.current || !mapContainerRef.current || map) return;

    const L = leafletRef.current;

    // Set up map
    const mapInstance = L.map(mapContainerRef.current).setView(
      mapCenter,
      mapZoom
    );

    // Add tile layer
    L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
      attribution:
        '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
    }).addTo(mapInstance);

    // Add marker cluster group if enabled
    if (clusterMarkers) {
      markerClusterRef.current = L.markerClusterGroup();
      mapInstance.addLayer(markerClusterRef.current);
    }

    // Get user location if allowed
    try {
      if (navigator.geolocation) {
        navigator.geolocation.getCurrentPosition(
          position => {
            const userPos = [
              position.coords.latitude,
              position.coords.longitude,
            ];
            setUserLocation(userPos);

            // Add marker for user location
            const userIcon = L.divIcon({
              html: `<div style="background-color: #3a8eff; width: 12px; height: 12px; border-radius: 50%; border: 2px solid white;"></div>`,
              className: 'user-location-marker',
              iconSize: [16, 16],
              iconAnchor: [8, 8],
            });

            L.marker(userPos, { icon: userIcon })
              .addTo(mapInstance)
              .bindTooltip('Your Location', { permanent: false });
          },
          err => {
            console.warn('Geolocation error:', err);
          }
        );
      }
    } catch (geoError) {
      console.warn('Geolocation not available:', geoError);
    }

    setMap(mapInstance);
  };

  // Fetch weather stations
  useEffect(() => {
    if (!sdk) return;

    const fetchStations = async () => {
      try {
        setLoading(true);
        setError(null);

        // Fetch weather stations
        const params = {
          active: true,
        };

        if (region) {
          params.region = region;
        }

        if (dataTypes && Array.isArray(dataTypes) && dataTypes.length > 0) {
          params.dataTypes = dataTypes;
        }

        const stationData = await sdk.getWeatherStations(params);

        if (stationData && Array.isArray(stationData)) {
          setStations(stationData);
          setFilteredStations(stationData);
        } else {
          setStations([]);
          setFilteredStations([]);
        }
      } catch (err) {
        console.error('Error fetching weather stations:', err);
        setError('Failed to load weather stations. Please try again later.');
      } finally {
        setLoading(false);
      }
    };

    fetchStations();
  }, [sdk, region, dataTypes]);

  // Filter stations when filter changes
  useEffect(() => {
    if (!filter) {
      setFilteredStations(stations);
      return;
    }

    const lowerFilter = filter.toLowerCase();
    const filtered = stations.filter(
      station =>
        station.name.toLowerCase().includes(lowerFilter) ||
        station.id.toLowerCase().includes(lowerFilter) ||
        (station.location &&
          station.location.toLowerCase().includes(lowerFilter)) ||
        (station.region && station.region.toLowerCase().includes(lowerFilter))
    );

    setFilteredStations(filtered);
  }, [filter, stations]);

  // Update markers when filtered stations change
  useEffect(() => {
    if (!map || !leafletRef.current || !filteredStations.length) return;

    const L = leafletRef.current;

    // Clear existing markers
    markers.forEach(marker => {
      if (clusterMarkers && markerClusterRef.current) {
        markerClusterRef.current.removeLayer(marker);
      } else {
        map.removeLayer(marker);
      }
    });

    // Create new markers
    const newMarkers = filteredStations
      .map(station => {
        if (!station.latitude || !station.longitude) return null;

        // Create marker icon based on station type and reliability
        const getMarkerColor = station => {
          if (!station.reliability) return '#999999';

          if (station.reliability >= 90) return '#4caf50';
          if (station.reliability >= 75) return '#8bc34a';
          if (station.reliability >= 50) return '#ffeb3b';
          if (station.reliability >= 25) return '#ff9800';
          return '#f44336';
        };

        const isSelected = selectedStation === station.id;
        const color = getMarkerColor(station);

        const icon = L.divIcon({
          html: `<div style="background-color: ${color}; width: ${isSelected ? '14px' : '10px'}; height: ${isSelected ? '14px' : '10px'}; border-radius: 50%; border: ${isSelected ? '3px' : '2px'} solid white;"></div>`,
          className: 'weather-station-marker',
          iconSize: [isSelected ? 20 : 14, isSelected ? 20 : 14],
          iconAnchor: [isSelected ? 10 : 7, isSelected ? 10 : 7],
        });

        // Create marker
        const marker = L.marker([station.latitude, station.longitude], {
          icon,
        });

        // Add popup if enabled
        if (showPopup) {
          marker.bindPopup(`
          <div style="min-width: 150px;">
            <strong>${station.name}</strong><br>
            <small>${station.id}</small><br>
            ${station.location ? `<small>${station.location}</small><br>` : ''}
            ${
              station.dataTypes && Array.isArray(station.dataTypes)
                ? `
              <div style="margin-top: 5px;">
                <small><strong>Data Types:</strong> ${station.dataTypes.join(', ')}</small>
              </div>
            `
                : ''
            }
            ${
              station.lastUpdated
                ? `
              <div style="margin-top: 5px;">
                <small><strong>Last Updated:</strong> ${new Date(station.lastUpdated).toLocaleString()}</small>
              </div>
            `
                : ''
            }
            <div style="margin-top: 8px;">
              <button 
                style="padding: 4px 8px; background-color: #3a8eff; color: white; border: none; border-radius: 4px; cursor: pointer; font-size: 12px;"
                onclick="document.dispatchEvent(new CustomEvent('select-station', { detail: '${station.id}' }))"
              >
                Select Station
              </button>
            </div>
          </div>
        `);
        } else {
          marker.bindTooltip(station.name, { permanent: false });
        }

        // Add click handler
        marker.on('click', () => {
          handleStationSelect(station);
        });

        // Add marker to map or cluster
        if (clusterMarkers && markerClusterRef.current) {
          markerClusterRef.current.addLayer(marker);
        } else {
          marker.addTo(map);
        }

        return marker;
      })
      .filter(Boolean);

    setMarkers(newMarkers);

    // Fit bounds if there are markers
    if (newMarkers.length > 0) {
      const group = L.featureGroup(newMarkers);
      map.fitBounds(group.getBounds(), { padding: [50, 50] });
    }
  }, [map, filteredStations, selectedStation, clusterMarkers]);

  // Handle station selection
  const handleStationSelect = station => {
    setSelectedStation(station.id);

    if (map && station.latitude && station.longitude) {
      map.setView(
        [station.latitude, station.longitude],
        map.getZoom() < 10 ? 10 : map.getZoom()
      );
    }

    if (onStationSelect && typeof onStationSelect === 'function') {
      onStationSelect(station);
    }
  };

  // Set up custom event listener
  useEffect(() => {
    const handleSelectStationEvent = e => {
      const stationId = e.detail;
      const station = stations.find(s => s.id === stationId);
      if (station) {
        handleStationSelect(station);
      }
    };

    document.addEventListener('select-station', handleSelectStationEvent);

    return () => {
      document.removeEventListener('select-station', handleSelectStationEvent);
    };
  }, [stations]);

  // Handle filter change
  const handleFilterChange = e => {
    setFilter(e.target.value);
  };

  // Handle map reset
  const handleResetMap = () => {
    if (map) {
      map.setView(mapCenter, mapZoom);
    }
  };

  // Handle find me
  const handleFindMe = () => {
    if (map && userLocation) {
      map.setView(userLocation, 12);
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
      marginBottom: '8px',
    },
    mapContainer: {
      height: `${height}px`,
      width: '100%',
      position: 'relative',
      borderRadius: '4px',
      overflow: 'hidden',
    },
    controls: {
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      flexWrap: 'wrap',
      gap: '8px',
      marginBottom: '16px',
    },
    search: {
      flex: '1',
      minWidth: '200px',
    },
    searchInput: {
      padding: '8px 12px',
      borderRadius: '4px',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      backgroundColor: theme === 'dark' ? '#444' : '#fff',
      color: theme === 'dark' ? '#fff' : '#333',
      fontSize: '14px',
      width: '100%',
    },
    buttonGroup: {
      display: 'flex',
      gap: '8px',
    },
    button: {
      padding: '6px 12px',
      backgroundColor: theme === 'dark' ? '#3a8eff' : '#3a8eff',
      color: '#fff',
      border: 'none',
      borderRadius: '4px',
      cursor: 'pointer',
      fontSize: '14px',
      fontWeight: '500',
    },
    buttonSecondary: {
      padding: '6px 12px',
      backgroundColor: theme === 'dark' ? '#444' : '#f5f5f5',
      color: theme === 'dark' ? '#fff' : '#333',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      borderRadius: '4px',
      cursor: 'pointer',
      fontSize: '14px',
    },
    loading: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
      backgroundColor: 'rgba(0, 0, 0, 0.5)',
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      color: '#fff',
      fontSize: '16px',
      zIndex: 1000,
    },
    error: {
      padding: '16px',
      textAlign: 'center',
      color: theme === 'dark' ? '#ff8a8a' : '#cc0000',
      fontSize: '14px',
    },
    info: {
      marginTop: '12px',
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
  };

  // Custom styles for Leaflet
  useEffect(() => {
    // Apply custom styles for leaflet elements
    if (typeof document !== 'undefined') {
      // Create style element if it doesn't exist
      let styleElement = document.getElementById('wrfcoin-leaflet-styles');

      if (!styleElement) {
        styleElement = document.createElement('style');
        styleElement.id = 'wrfcoin-leaflet-styles';
        document.head.appendChild(styleElement);
      }

      // Set theme-based styles
      styleElement.innerHTML = `
        .leaflet-container {
          background-color: ${theme === 'dark' ? '#333' : '#f5f5f5'};
        }
        .leaflet-popup-content-wrapper {
          background-color: ${theme === 'dark' ? '#333' : '#fff'};
          color: ${theme === 'dark' ? '#fff' : '#333'};
        }
        .leaflet-popup-tip {
          background-color: ${theme === 'dark' ? '#333' : '#fff'};
        }
      `;
    }
  }, [theme]);

  return (
    <div style={styles.container} className="weather-station-map">
      <div style={styles.header}>
        <div style={styles.title}>Weather Station Map</div>
      </div>

      {showControls && (
        <div style={styles.controls}>
          <div style={styles.search}>
            <input
              type="text"
              style={styles.searchInput}
              placeholder="Search stations..."
              value={filter}
              onChange={handleFilterChange}
            />
          </div>

          <div style={styles.buttonGroup}>
            {userLocation && (
              <button style={styles.button} onClick={handleFindMe}>
                Find Me
              </button>
            )}

            <button style={styles.buttonSecondary} onClick={handleResetMap}>
              Reset Map
            </button>
          </div>
        </div>
      )}

      <div style={styles.mapContainer}>
        <div
          ref={mapContainerRef}
          style={{ width: '100%', height: '100%' }}
        ></div>

        {loading && <div style={styles.loading}>Loading map data...</div>}
      </div>

      {error && <div style={styles.error}>{error}</div>}

      <div style={styles.info}>
        {filteredStations.length > 0
          ? `${filteredStations.length} stations found`
          : 'No stations found'}
        {selectedStation &&
          ` • Selected: ${stations.find(s => s.id === selectedStation)?.name || selectedStation}`}
      </div>
    </div>
  );
};

WeatherStationMap.propTypes = {
  onStationSelect: PropTypes.func,
  region: PropTypes.string,
  dataTypes: PropTypes.arrayOf(PropTypes.string),
  theme: PropTypes.oneOf(['light', 'dark']),
  initialSelected: PropTypes.string,
  mapCenter: PropTypes.arrayOf(PropTypes.number),
  mapZoom: PropTypes.number,
  height: PropTypes.number,
  showPopup: PropTypes.bool,
  showControls: PropTypes.bool,
  clusterMarkers: PropTypes.bool,
};

export default WeatherStationMap;
