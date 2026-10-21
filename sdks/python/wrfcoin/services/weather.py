"""
Wrfcoin Python SDK - Weather Service

Weather data validation, computational results, and environmental tracking.
"""

from typing import Dict, Any, List, Optional
from datetime import datetime, timedelta
from ..exceptions import (
    ValidationError,
    WeatherDataError,
    InvalidAddressError,
    TransactionError
)


class WeatherService:
    """Service for weather data operations and validation"""
    
    def __init__(self, graphql_client):
        self.graphql = graphql_client
    
    async def get_weather_data(
        self,
        location: str,
        start_date: Optional[str] = None,
        end_date: Optional[str] = None,
        data_type: str = "all"
    ) -> Dict[str, Any]:
        """Get weather data for a location and time range"""
        if not location:
            raise ValidationError("Location is required")
        
        # Validate date formats
        if start_date and not self._is_valid_date(start_date):
            raise ValidationError(f"Invalid start date format: {start_date}")
        
        if end_date and not self._is_valid_date(end_date):
            raise ValidationError(f"Invalid end date format: {end_date}")
        
        query = """
        query GetWeatherData($location: String!, $startDate: String, $endDate: String, $dataType: String!) {
            weatherData(location: $location, startDate: $startDate, endDate: $endDate, dataType: $dataType) {
                location {
                    name
                    coordinates {
                        latitude
                        longitude
                    }
                    timezone
                    elevation
                }
                timeRange {
                    startDate
                    endDate
                    totalDays
                }
                observations {
                    timestamp
                    temperature
                    humidity
                    pressure
                    windSpeed
                    windDirection
                    precipitation
                    visibility
                    cloudCover
                    uvIndex
                    airQuality {
                        aqi
                        pm25
                        pm10
                        co
                        no2
                        so2
                        o3
                    }
                }
                statistics {
                    avgTemperature
                    maxTemperature
                    minTemperature
                    totalPrecipitation
                    avgHumidity
                    avgPressure
                }
                verification {
                    isValidated
                    validatorAddress
                    validationScore
                    consensusReached
                    validators {
                        address
                        score
                        stake
                    }
                }
            }
        }
        """
        
        variables = {
            "location": location,
            "dataType": data_type
        }
        
        if start_date:
            variables["startDate"] = start_date
        if end_date:
            variables["endDate"] = end_date
        
        result = await self.graphql.query(query, variables)
        weather_data = result.get('weatherData')
        
        if not weather_data:
            raise WeatherDataError(f"No weather data found for location: {location}")
        
        return weather_data
    
    async def submit_weather_observation(
        self,
        observer_address: str,
        location: Dict[str, Any],
        observation: Dict[str, Any],
        metadata: Optional[Dict[str, Any]] = None
    ) -> Dict[str, Any]:
        """Submit weather observation for validation"""
        if not self._is_valid_address(observer_address):
            raise InvalidAddressError(f"Invalid observer address: {observer_address}")
        
        # Validate required observation fields
        required_fields = ['timestamp', 'temperature', 'humidity', 'pressure']
        for field in required_fields:
            if field not in observation:
                raise ValidationError(f"Observation field '{field}' is required")
        
        # Validate location fields
        location_fields = ['latitude', 'longitude', 'name']
        for field in location_fields:
            if field not in location:
                raise ValidationError(f"Location field '{field}' is required")
        
        # Validate coordinate ranges
        lat = location.get('latitude')
        lon = location.get('longitude')
        
        if not (-90 <= lat <= 90):
            raise ValidationError(f"Invalid latitude: {lat}")
        
        if not (-180 <= lon <= 180):
            raise ValidationError(f"Invalid longitude: {lon}")
        
        mutation = """
        mutation SubmitWeatherObservation(
            $observerAddress: String!,
            $location: LocationInput!,
            $observation: WeatherObservationInput!,
            $metadata: JSON
        ) {
            submitWeatherObservation(
                observerAddress: $observerAddress,
                location: $location,
                observation: $observation,
                metadata: $metadata
            ) {
                observationId
                transactionHash
                status
                validationScore
                rewardAmount
                validators {
                    address
                    score
                    stake
                }
                consensusReached
            }
        }
        """
        
        variables = {
            "observerAddress": observer_address,
            "location": location,
            "observation": observation,
            "metadata": metadata
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('submitWeatherObservation', {})
        except Exception as error:
            raise WeatherDataError(f"Failed to submit weather observation: {error}")
    
    async def validate_weather_data(
        self,
        validator_address: str,
        observation_id: str,
        validation_score: float,
        validation_details: Dict[str, Any]
    ) -> Dict[str, Any]:
        """Validate submitted weather data"""
        if not self._is_valid_address(validator_address):
            raise InvalidAddressError(f"Invalid validator address: {validator_address}")
        
        if not observation_id:
            raise ValidationError("Observation ID is required")
        
        if not (0 <= validation_score <= 100):
            raise ValidationError("Validation score must be between 0 and 100")
        
        mutation = """
        mutation ValidateWeatherData(
            $validatorAddress: String!,
            $observationId: String!,
            $validationScore: Float!,
            $validationDetails: ValidationDetailsInput!
        ) {
            validateWeatherData(
                validatorAddress: $validatorAddress,
                observationId: $observationId,
                validationScore: $validationScore,
                validationDetails: $validationDetails
            ) {
                validationId
                transactionHash
                status
                validatorReward
                observationFinalScore
                consensusReached
            }
        }
        """
        
        variables = {
            "validatorAddress": validator_address,
            "observationId": observation_id,
            "validationScore": validation_score,
            "validationDetails": validation_details
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('validateWeatherData', {})
        except Exception as error:
            raise WeatherDataError(f"Failed to validate weather data: {error}")
    
    async def get_weather_stations(
        self,
        location: Optional[Dict[str, Any]] = None,
        radius: Optional[float] = None,
        limit: int = 20,
        offset: int = 0
    ) -> List[Dict[str, Any]]:
        """Get weather stations in an area"""
        if limit <= 0 or limit > 100:
            raise ValidationError("Limit must be between 1 and 100")
        
        if location:
            lat = location.get('latitude')
            lon = location.get('longitude')
            
            if lat is None or lon is None:
                raise ValidationError("Location must include latitude and longitude")
            
            if not (-90 <= lat <= 90):
                raise ValidationError(f"Invalid latitude: {lat}")
            
            if not (-180 <= lon <= 180):
                raise ValidationError(f"Invalid longitude: {lon}")
        
        query = """
        query GetWeatherStations($location: LocationInput, $radius: Float, $limit: Int!, $offset: Int!) {
            weatherStations(location: $location, radius: $radius, limit: $limit, offset: $offset) {
                stations {
                    stationId
                    name
                    location {
                        latitude
                        longitude
                        elevation
                    }
                    operator
                    operatorAddress
                    isActive
                    lastObservation
                    totalObservations
                    validationScore
                    equipment {
                        sensors
                        model
                        calibrationDate
                    }
                    rewards {
                        totalEarned
                        avgRewardPerObservation
                    }
                }
                totalCount
                hasMore
            }
        }
        """
        
        variables = {
            "limit": limit,
            "offset": offset
        }
        
        if location:
            variables["location"] = location
        if radius:
            variables["radius"] = radius
        
        result = await self.graphql.query(query, variables)
        stations_data = result.get('weatherStations', {})
        return stations_data.get('stations', [])
    
    async def register_weather_station(
        self,
        operator_address: str,
        station_info: Dict[str, Any],
        equipment_details: Dict[str, Any],
        stake_amount: str
    ) -> Dict[str, Any]:
        """Register a new weather station"""
        if not self._is_valid_address(operator_address):
            raise InvalidAddressError(f"Invalid operator address: {operator_address}")
        
        # Validate required station info
        required_fields = ['name', 'location', 'description']
        for field in required_fields:
            if field not in station_info:
                raise ValidationError(f"Station info field '{field}' is required")
        
        # Validate location
        location = station_info['location']
        lat = location.get('latitude')
        lon = location.get('longitude')
        
        if lat is None or lon is None:
            raise ValidationError("Station location must include latitude and longitude")
        
        if not (-90 <= lat <= 90):
            raise ValidationError(f"Invalid latitude: {lat}")
        
        if not (-180 <= lon <= 180):
            raise ValidationError(f"Invalid longitude: {lon}")
        
        try:
            stake = float(stake_amount)
            if stake <= 0:
                raise ValidationError("Stake amount must be positive")
        except (ValueError, TypeError):
            raise ValidationError(f"Invalid stake amount: {stake_amount}")
        
        mutation = """
        mutation RegisterWeatherStation(
            $operatorAddress: String!,
            $stationInfo: WeatherStationInput!,
            $equipmentDetails: EquipmentDetailsInput!,
            $stakeAmount: String!
        ) {
            registerWeatherStation(
                operatorAddress: $operatorAddress,
                stationInfo: $stationInfo,
                equipmentDetails: $equipmentDetails,
                stakeAmount: $stakeAmount
            ) {
                stationId
                transactionHash
                status
                registrationFee
                requiredStake
                approvalPending
            }
        }
        """
        
        variables = {
            "operatorAddress": operator_address,
            "stationInfo": station_info,
            "equipmentDetails": equipment_details,
            "stakeAmount": stake_amount
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('registerWeatherStation', {})
        except Exception as error:
            raise WeatherDataError(f"Failed to register weather station: {error}")
    
    async def get_computational_results(
        self,
        model_id: str,
        location: Optional[str] = None,
        start_date: Optional[str] = None,
        end_date: Optional[str] = None
    ) -> Dict[str, Any]:
        """Get computational weather modeling results"""
        if not model_id:
            raise ValidationError("Model ID is required")
        
        query = """
        query GetComputationalResults($modelId: String!, $location: String, $startDate: String, $endDate: String) {
            computationalResults(modelId: $modelId, location: $location, startDate: $startDate, endDate: $endDate) {
                modelId
                modelName
                modelVersion
                computationId
                inputData {
                    initialConditions
                    boundaries
                    parameters
                }
                results {
                    timestamp
                    location
                    predictions {
                        temperature
                        precipitation
                        windSpeed
                        pressure
                        humidity
                    }
                    confidence
                    uncertainty
                }
                performance {
                    executionTime
                    accuracy
                    energyUsed
                    carbonFootprint
                }
                validation {
                    isValidated
                    validationScore
                    validators
                }
                nftTokenId
                carbonCredits
            }
        }
        """
        
        variables = {"modelId": model_id}
        if location:
            variables["location"] = location
        if start_date:
            variables["startDate"] = start_date
        if end_date:
            variables["endDate"] = end_date
        
        result = await self.graphql.query(query, variables)
        return result.get('computationalResults', {})
    
    async def run_weather_computation(
        self,
        requester_address: str,
        model_id: str,
        input_parameters: Dict[str, Any],
        computation_type: str = "forecast"
    ) -> Dict[str, Any]:
        """Request weather computation with specified model"""
        if not self._is_valid_address(requester_address):
            raise InvalidAddressError(f"Invalid requester address: {requester_address}")
        
        if not model_id:
            raise ValidationError("Model ID is required")
        
        if not input_parameters:
            raise ValidationError("Input parameters are required")
        
        mutation = """
        mutation RunWeatherComputation(
            $requesterAddress: String!,
            $modelId: String!,
            $inputParameters: ComputationInput!,
            $computationType: String!
        ) {
            runWeatherComputation(
                requesterAddress: $requesterAddress,
                modelId: $modelId,
                inputParameters: $inputParameters,
                computationType: $computationType
            ) {
                computationId
                transactionHash
                status
                estimatedCompletionTime
                costEstimate
                carbonFootprintEstimate
                queuePosition
            }
        }
        """
        
        variables = {
            "requesterAddress": requester_address,
            "modelId": model_id,
            "inputParameters": input_parameters,
            "computationType": computation_type
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('runWeatherComputation', {})
        except Exception as error:
            raise WeatherDataError(f"Failed to run weather computation: {error}")
    
    async def subscribe_to_weather_updates(self, location: str, callback) -> callable:
        """Subscribe to real-time weather updates for a location"""
        if not location:
            raise ValidationError("Location is required")
        
        subscription = """
        subscription WeatherUpdates($location: String!) {
            weatherUpdates(location: $location) {
                timestamp
                location
                observation {
                    temperature
                    humidity
                    pressure
                    windSpeed
                    precipitation
                }
                validationStatus
                observerAddress
                stationId
            }
        }
        """
        
        return await self.graphql.subscribe(
            subscription,
            variables={"location": location},
            on_data=callback
        )
    
    def _is_valid_address(self, address: str) -> bool:
        """Validate Ethereum-style address format"""
        if not isinstance(address, str):
            return False
        
        # Remove 0x prefix if present
        addr = address.lower()
        if addr.startswith('0x'):
            addr = addr[2:]
        
        # Check if it's 40 hex characters
        if len(addr) != 40:
            return False
        
        try:
            int(addr, 16)
            return True
        except ValueError:
            return False
    
    def _is_valid_date(self, date_str: str) -> bool:
        """Validate ISO date format"""
        try:
            datetime.fromisoformat(date_str.replace('Z', '+00:00'))
            return True
        except ValueError:
            return False
    
    def format_temperature(self, celsius: float, unit: str = "C") -> float:
        """Convert temperature between units"""
        if unit.upper() == "F":
            return (celsius * 9/5) + 32
        elif unit.upper() == "K":
            return celsius + 273.15
        else:
            return celsius
    
    def validate_sensor_reading(self, reading: Dict[str, Any]) -> bool:
        """Validate sensor reading values are within reasonable ranges"""
        validations = {
            'temperature': (-100, 60),  # Celsius
            'humidity': (0, 100),       # Percentage
            'pressure': (800, 1200),    # hPa
            'windSpeed': (0, 200),      # km/h
            'precipitation': (0, 500),   # mm
            'visibility': (0, 50),       # km
            'cloudCover': (0, 100),     # Percentage
            'uvIndex': (0, 15)          # UV Index
        }
        
        for field, (min_val, max_val) in validations.items():
            if field in reading:
                value = reading[field]
                if not isinstance(value, (int, float)) or not (min_val <= value <= max_val):
                    return False
        
        return True