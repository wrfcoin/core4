import React, { useState, useEffect } from 'react';
import PropTypes from 'prop-types';
import { useWrfcoinContext } from '../../context/WrfcoinContext';

/**
 * WeatherStationList component for displaying and filtering weather stations
 *
 * This component provides a list of weather stations with filtering, sorting,
 * and selection capabilities.
 */
const WeatherStationList = ({
  onStationSelect,
  region = null,
  dataTypes = null,
  maxItems = 10,
  theme = 'light',
  listType = 'grid',
  showMap = false,
  initialSelected = null,
  sortBy = 'distance',
}) => {
  const { sdk } = useWrfcoinContext();
  const [stations, setStations] = useState([]);
  const [filteredStations, setFilteredStations] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [selectedStation, setSelectedStation] = useState(initialSelected);
  const [filter, setFilter] = useState('');
  const [userLocation, setUserLocation] = useState(null);
  const [currentPage, setCurrentPage] = useState(1);
  const [sortOrder, setSortOrder] = useState(sortBy);

  // Fetch weather stations
  useEffect(() => {
    if (!sdk) return;

    const fetchStations = async () => {
      try {
        setLoading(true);
        setError(null);

        // Get user location for distance calculations if allowed
        try {
          if (navigator.geolocation) {
            navigator.geolocation.getCurrentPosition(
              position => {
                setUserLocation({
                  latitude: position.coords.latitude,
                  longitude: position.coords.longitude,
                });
              },
              err => {
                console.warn('Geolocation error:', err);
              },
              { timeout: 10000 }
            );
          }
        } catch (geoError) {
          console.warn('Geolocation not available:', geoError);
        }

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

  // Filter and sort stations when filter, sort, or stations change
  useEffect(() => {
    // Filter stations based on search text
    let filtered = stations;

    if (filter) {
      const lowerFilter = filter.toLowerCase();
      filtered = stations.filter(
        station =>
          station.name.toLowerCase().includes(lowerFilter) ||
          station.id.toLowerCase().includes(lowerFilter) ||
          (station.location &&
            station.location.toLowerCase().includes(lowerFilter)) ||
          (station.region && station.region.toLowerCase().includes(lowerFilter))
      );
    }

    // Sort stations
    filtered = [...filtered].sort((a, b) => {
      switch (sortOrder) {
        case 'name':
          return a.name.localeCompare(b.name);
        case 'date':
          return new Date(b.lastUpdated) - new Date(a.lastUpdated);
        case 'reliability':
          return (b.reliability || 0) - (a.reliability || 0);
        case 'distance':
          if (!userLocation) return 0;

          const distA = calculateDistance(
            userLocation.latitude,
            userLocation.longitude,
            a.latitude,
            a.longitude
          );

          const distB = calculateDistance(
            userLocation.latitude,
            userLocation.longitude,
            b.latitude,
            b.longitude
          );

          return distA - distB;
        default:
          return 0;
      }
    });

    setFilteredStations(filtered);
  }, [stations, filter, sortOrder, userLocation]);

  // Calculate distance between two points using Haversine formula
  const calculateDistance = (lat1, lon1, lat2, lon2) => {
    if (!lat1 || !lon1 || !lat2 || !lon2) return Infinity;

    const R = 6371; // Earth's radius in km
    const dLat = ((lat2 - lat1) * Math.PI) / 180;
    const dLon = ((lon2 - lon1) * Math.PI) / 180;

    const a =
      Math.sin(dLat / 2) * Math.sin(dLat / 2) +
      Math.cos((lat1 * Math.PI) / 180) *
        Math.cos((lat2 * Math.PI) / 180) *
        Math.sin(dLon / 2) *
        Math.sin(dLon / 2);

    const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
    return R * c;
  };

  // Handle station selection
  const handleStationSelect = station => {
    setSelectedStation(station.id);

    if (onStationSelect && typeof onStationSelect === 'function') {
      onStationSelect(station);
    }
  };

  // Handle filter change
  const handleFilterChange = e => {
    setFilter(e.target.value);
    setCurrentPage(1);
  };

  // Handle sort change
  const handleSortChange = value => {
    setSortOrder(value);
    setCurrentPage(1);
  };

  // Get items for current page
  const getCurrentPageItems = () => {
    const startIndex = (currentPage - 1) * maxItems;
    const endIndex = startIndex + maxItems;

    return filteredStations.slice(startIndex, endIndex);
  };

  // Format date for display
  const formatDate = dateString => {
    if (!dateString) return 'Unknown';

    const date = new Date(dateString);
    return date.toLocaleString();
  };

  // Format distance for display
  const formatDistance = station => {
    if (!userLocation || !station.latitude || !station.longitude) {
      return 'Unknown';
    }

    const distance = calculateDistance(
      userLocation.latitude,
      userLocation.longitude,
      station.latitude,
      station.longitude
    );

    if (distance < 1) {
      return `${Math.round(distance * 1000)} m`;
    } else {
      return `${Math.round(distance * 10) / 10} km`;
    }
  };

  // Get reliability color
  const getReliabilityColor = reliability => {
    if (reliability === undefined || reliability === null) return '#999';

    if (reliability >= 90) return '#4caf50';
    if (reliability >= 75) return '#8bc34a';
    if (reliability >= 50) return '#ffeb3b';
    if (reliability >= 25) return '#ff9800';
    return '#f44336';
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
    sortContainer: {
      display: 'flex',
      alignItems: 'center',
      gap: '8px',
    },
    sortLabel: {
      fontSize: '14px',
      whiteSpace: 'nowrap',
    },
    sortSelect: {
      padding: '8px 12px',
      borderRadius: '4px',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      backgroundColor: theme === 'dark' ? '#444' : '#f5f5f5',
      color: theme === 'dark' ? '#fff' : '#333',
      fontSize: '14px',
    },
    viewToggle: {
      display: 'flex',
      gap: '8px',
    },
    viewButton: active => ({
      padding: '6px 12px',
      backgroundColor: active
        ? theme === 'dark'
          ? '#3a8eff'
          : '#3a8eff'
        : theme === 'dark'
          ? '#444'
          : '#f5f5f5',
      color: active ? '#fff' : theme === 'dark' ? '#fff' : '#333',
      border: theme === 'dark' ? '1px solid #555' : '1px solid #ddd',
      borderRadius: '4px',
      cursor: 'pointer',
      fontSize: '14px',
    }),
    gridContainer: {
      display: 'grid',
      gridTemplateColumns: 'repeat(auto-fill, minmax(250px, 1fr))',
      gap: '16px',
    },
    listContainer: {
      display: 'flex',
      flexDirection: 'column',
      gap: '8px',
    },
    gridItem: isSelected => ({
      backgroundColor: isSelected
        ? theme === 'dark'
          ? '#1c4373'
          : '#e3f2fd'
        : theme === 'dark'
          ? '#222'
          : '#f9f9f9',
      border: isSelected
        ? theme === 'dark'
          ? '1px solid #3a8eff'
          : '1px solid #3a8eff'
        : theme === 'dark'
          ? '1px solid #444'
          : '1px solid #ddd',
      borderRadius: '4px',
      padding: '16px',
      cursor: 'pointer',
      transition: 'background-color 0.2s ease',
      '&:hover': {
        backgroundColor: theme === 'dark' ? '#2a2a2a' : '#f5f5f5',
      },
    }),
    listItem: isSelected => ({
      backgroundColor: isSelected
        ? theme === 'dark'
          ? '#1c4373'
          : '#e3f2fd'
        : theme === 'dark'
          ? '#222'
          : '#f9f9f9',
      border: isSelected
        ? theme === 'dark'
          ? '1px solid #3a8eff'
          : '1px solid #3a8eff'
        : theme === 'dark'
          ? '1px solid #444'
          : '1px solid #ddd',
      borderRadius: '4px',
      padding: '12px',
      cursor: 'pointer',
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      transition: 'background-color 0.2s ease',
      '&:hover': {
        backgroundColor: theme === 'dark' ? '#2a2a2a' : '#f5f5f5',
      },
    }),
    stationName: {
      fontSize: '16px',
      fontWeight: '500',
      marginBottom: '4px',
    },
    stationId: {
      fontSize: '12px',
      color: theme === 'dark' ? '#aaa' : '#777',
      marginBottom: '8px',
    },
    stationInfo: {
      display: 'flex',
      flexDirection: 'column',
      gap: '4px',
      fontSize: '14px',
    },
    stationLocation: {
      display: 'flex',
      alignItems: 'center',
      gap: '4px',
      fontSize: '14px',
    },
    infoItem: {
      display: 'flex',
      alignItems: 'center',
      gap: '4px',
      fontSize: '13px',
      color: theme === 'dark' ? '#ddd' : '#555',
    },
    reliabilityIndicator: reliability => ({
      width: '8px',
      height: '8px',
      borderRadius: '50%',
      backgroundColor: getReliabilityColor(reliability),
    }),
    pagination: {
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      marginTop: '16px',
    },
    pageInfo: {
      fontSize: '14px',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    pageButtons: {
      display: 'flex',
      gap: '8px',
    },
    pageButton: isActive => ({
      padding: '6px 12px',
      backgroundColor: isActive
        ? theme === 'dark'
          ? '#3a8eff'
          : '#3a8eff'
        : theme === 'dark'
          ? '#444'
          : '#f5f5f5',
      color: isActive ? '#fff' : theme === 'dark' ? '#fff' : '#333',
      border: 'none',
      borderRadius: '4px',
      cursor: isActive ? 'default' : 'pointer',
      fontSize: '14px',
      opacity: isActive ? 1 : 0.8,
    }),
    noResults: {
      padding: '16px',
      textAlign: 'center',
      color: theme === 'dark' ? '#aaa' : '#777',
      fontSize: '14px',
    },
    loading: {
      padding: '16px',
      textAlign: 'center',
      color: theme === 'dark' ? '#aaa' : '#777',
    },
    error: {
      padding: '16px',
      textAlign: 'center',
      color: theme === 'dark' ? '#ff8a8a' : '#cc0000',
      fontSize: '14px',
    },
    dataTypeTag: type => ({
      display: 'inline-block',
      padding: '2px 6px',
      borderRadius: '4px',
      fontSize: '11px',
      backgroundColor: getDataTypeColor(type, theme),
      color: '#fff',
      marginRight: '4px',
      marginBottom: '4px',
    }),
    dataTypes: {
      display: 'flex',
      flexWrap: 'wrap',
      marginTop: '8px',
    },
    listItemContent: {
      flex: 1,
    },
    listItemDistance: {
      fontSize: '14px',
      fontWeight: '500',
      color: theme === 'dark' ? '#ddd' : '#555',
    },
  };

  // Get color for data type tag
  function getDataTypeColor(type, themeMode) {
    switch (type) {
      case 'temperature':
        return '#ff9800';
      case 'precipitation':
        return '#2196f3';
      case 'humidity':
        return '#03a9f4';
      case 'pressure':
        return '#673ab7';
      case 'windSpeed':
      case 'windDirection':
        return '#4caf50';
      case 'solarRadiation':
        return '#ffc107';
      case 'airQuality':
        return '#9c27b0';
      default:
        return themeMode === 'dark' ? '#555' : '#999';
    }
  }

  // Get user-friendly name for data type
  function getDataTypeName(type) {
    switch (type) {
      case 'temperature':
        return 'Temp';
      case 'precipitation':
        return 'Precip';
      case 'humidity':
        return 'Humid';
      case 'pressure':
        return 'Press';
      case 'windSpeed':
        return 'Wind';
      case 'windDirection':
        return 'Wind Dir';
      case 'solarRadiation':
        return 'Solar';
      case 'airQuality':
        return 'Air';
      default:
        return type;
    }
  }

  // Render grid item
  const renderGridItem = station => {
    const isSelected = selectedStation === station.id;

    return (
      <div
        key={station.id}
        style={styles.gridItem(isSelected)}
        onClick={() => handleStationSelect(station)}
        className="weather-station-grid-item"
      >
        <div style={styles.stationName}>{station.name}</div>
        <div style={styles.stationId}>ID: {station.id}</div>

        <div style={styles.stationInfo}>
          <div style={styles.infoItem}>
            <span>Location:</span> {station.location || 'Unknown'}
          </div>

          <div style={styles.infoItem}>
            <span>Last Updated:</span> {formatDate(station.lastUpdated)}
          </div>

          <div style={styles.infoItem}>
            <span>Reliability:</span>
            <div style={styles.reliabilityIndicator(station.reliability)}></div>
            {station.reliability ? `${station.reliability}%` : 'Unknown'}
          </div>

          <div style={styles.infoItem}>
            <span>Distance:</span> {formatDistance(station)}
          </div>
        </div>

        {station.dataTypes && Array.isArray(station.dataTypes) && (
          <div style={styles.dataTypes}>
            {station.dataTypes.map(type => (
              <div key={type} style={styles.dataTypeTag(type)}>
                {getDataTypeName(type)}
              </div>
            ))}
          </div>
        )}
      </div>
    );
  };

  // Render list item
  const renderListItem = station => {
    const isSelected = selectedStation === station.id;

    return (
      <div
        key={station.id}
        style={styles.listItem(isSelected)}
        onClick={() => handleStationSelect(station)}
        className="weather-station-list-item"
      >
        <div style={styles.listItemContent}>
          <div style={styles.stationName}>{station.name}</div>
          <div style={styles.stationId}>ID: {station.id}</div>

          <div style={styles.infoItem}>
            <span>Last Updated:</span> {formatDate(station.lastUpdated)}
          </div>

          {station.dataTypes && Array.isArray(station.dataTypes) && (
            <div style={styles.dataTypes}>
              {station.dataTypes.map(type => (
                <div key={type} style={styles.dataTypeTag(type)}>
                  {getDataTypeName(type)}
                </div>
              ))}
            </div>
          )}
        </div>

        <div style={styles.listItemDistance}>{formatDistance(station)}</div>
      </div>
    );
  };

  // Render pagination
  const renderPagination = () => {
    const totalPages = Math.ceil(filteredStations.length / maxItems);

    // Don't render pagination if there's only one page
    if (totalPages <= 1) return null;

    return (
      <div style={styles.pagination}>
        <div style={styles.pageInfo}>
          Showing {(currentPage - 1) * maxItems + 1} to{' '}
          {Math.min(currentPage * maxItems, filteredStations.length)} of{' '}
          {filteredStations.length} stations
        </div>

        <div style={styles.pageButtons}>
          <button
            style={styles.pageButton(false)}
            onClick={() => setCurrentPage(1)}
            disabled={currentPage === 1}
          >
            First
          </button>

          <button
            style={styles.pageButton(false)}
            onClick={() => setCurrentPage(currentPage - 1)}
            disabled={currentPage === 1}
          >
            Previous
          </button>

          {Array.from({ length: Math.min(5, totalPages) }, (_, i) => {
            let pageNum;
            if (totalPages <= 5) {
              pageNum = i + 1;
            } else if (currentPage <= 3) {
              pageNum = i + 1;
            } else if (currentPage >= totalPages - 2) {
              pageNum = totalPages - 4 + i;
            } else {
              pageNum = currentPage - 2 + i;
            }

            return (
              <button
                key={pageNum}
                style={styles.pageButton(currentPage === pageNum)}
                onClick={() => setCurrentPage(pageNum)}
              >
                {pageNum}
              </button>
            );
          })}

          <button
            style={styles.pageButton(false)}
            onClick={() => setCurrentPage(currentPage + 1)}
            disabled={currentPage === totalPages}
          >
            Next
          </button>

          <button
            style={styles.pageButton(false)}
            onClick={() => setCurrentPage(totalPages)}
            disabled={currentPage === totalPages}
          >
            Last
          </button>
        </div>
      </div>
    );
  };

  return (
    <div style={styles.container} className="weather-station-list">
      <div style={styles.header}>
        <div style={styles.title}>Weather Stations</div>
      </div>

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

        <div style={styles.sortContainer}>
          <div style={styles.sortLabel}>Sort by:</div>
          <select
            style={styles.sortSelect}
            value={sortOrder}
            onChange={e => handleSortChange(e.target.value)}
          >
            {userLocation && <option value="distance">Distance</option>}
            <option value="name">Name</option>
            <option value="date">Last Updated</option>
            <option value="reliability">Reliability</option>
          </select>
        </div>

        <div style={styles.viewToggle}>
          <button
            style={styles.viewButton(listType === 'grid')}
            onClick={() => setListType('grid')}
          >
            Grid
          </button>
          <button
            style={styles.viewButton(listType === 'list')}
            onClick={() => setListType('list')}
          >
            List
          </button>
        </div>
      </div>

      {loading ? (
        <div style={styles.loading}>Loading weather stations...</div>
      ) : error ? (
        <div style={styles.error}>{error}</div>
      ) : filteredStations.length === 0 ? (
        <div style={styles.noResults}>No weather stations found</div>
      ) : (
        <>
          <div
            style={
              listType === 'grid' ? styles.gridContainer : styles.listContainer
            }
          >
            {getCurrentPageItems().map(station =>
              listType === 'grid'
                ? renderGridItem(station)
                : renderListItem(station)
            )}
          </div>

          {renderPagination()}
        </>
      )}
    </div>
  );
};

WeatherStationList.propTypes = {
  onStationSelect: PropTypes.func,
  region: PropTypes.string,
  dataTypes: PropTypes.arrayOf(PropTypes.string),
  maxItems: PropTypes.number,
  theme: PropTypes.oneOf(['light', 'dark']),
  listType: PropTypes.oneOf(['grid', 'list']),
  showMap: PropTypes.bool,
  initialSelected: PropTypes.string,
  sortBy: PropTypes.oneOf(['name', 'date', 'reliability', 'distance']),
};

export default WeatherStationList;
