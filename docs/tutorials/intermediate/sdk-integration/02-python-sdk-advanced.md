# Advanced Python SDK Integration

Master advanced patterns and features of the WRFCoin Python SDK for climate research and data
science applications.

## What You'll Learn

- Advanced Python SDK configuration and optimization
- Scientific data processing and submission workflows
- Integration with popular data science libraries
- Automated climate data collection and analysis
- Machine learning integration with blockchain rewards
- Large-scale data processing patterns

## Prerequisites

- Completed [Getting Started tutorials](../../getting-started/)
- Strong Python knowledge (3.8+)
- Experience with pandas, numpy, and async programming
- Basic understanding of climate data formats

---

## 1. Advanced Configuration and Setup

### Production Data Science Environment

```python
# config/wrfcoin_manager.py
import asyncio
import logging
import os
import time
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import Dict, Optional, List, Any
import aiohttp
import pandas as pd
import numpy as np
from wrfcoin_sdk import WrfcoinClient
from wrfcoin_sdk.exceptions import (
    NetworkError, ValidationError, RateLimitError, InsufficientBalanceError
)

@dataclass
class PerformanceMetrics:
    total_requests: int = 0
    successful_requests: int = 0
    failed_requests: int = 0
    average_response_time: float = 0.0
    last_request_time: Optional[float] = None

@dataclass
class RetryConfig:
    max_retries: int = 3
    base_delay: float = 1.0
    max_delay: float = 60.0
    exponential_base: float = 2.0
    jitter: bool = True

class WrfcoinDataScienceManager:
    """Advanced WRFCoin client for data science applications"""

    def __init__(self, config: Optional[Dict[str, Any]] = None):
        self.config = self._build_config(config or {})
        self.client: Optional[WrfcoinClient] = None
        self.session: Optional[aiohttp.ClientSession] = None
        self.metrics = PerformanceMetrics()
        self.retry_config = RetryConfig(**self.config.get('retry', {}))
        self._connection_pool = None
        self._rate_limiter = None

        # Setup logging
        self._setup_logging()

    def _build_config(self, config: Dict[str, Any]) -> Dict[str, Any]:
        """Build configuration with environment variable fallbacks"""
        default_config = {
            'network': os.getenv('WRFCOIN_NETWORK', 'testnet'),
            'api_url': os.getenv('WRFCOIN_API_URL'),
            'api_key': os.getenv('WRFCOIN_API_KEY'),
            'max_concurrent_requests': int(os.getenv('WRFCOIN_MAX_CONCURRENT', '10')),
            'request_timeout': float(os.getenv('WRFCOIN_TIMEOUT', '30.0')),
            'cache_size': int(os.getenv('WRFCOIN_CACHE_SIZE', '1000')),
            'retry': {
                'max_retries': int(os.getenv('WRFCOIN_MAX_RETRIES', '3')),
                'base_delay': float(os.getenv('WRFCOIN_BASE_DELAY', '1.0')),
            },
            'data_science': {
                'chunk_size': int(os.getenv('WRFCOIN_CHUNK_SIZE', '1000')),
                'parallel_workers': int(os.getenv('WRFCOIN_WORKERS', '4')),
                'auto_validation': os.getenv('WRFCOIN_AUTO_VALIDATE', 'true').lower() == 'true',
                'compression': os.getenv('WRFCOIN_COMPRESSION', 'gzip'),
            }
        }

        # Deep merge with provided config
        return self._deep_merge(default_config, config)

    def _deep_merge(self, base: dict, override: dict) -> dict:
        """Deep merge two dictionaries"""
        result = base.copy()
        for key, value in override.items():
            if key in result and isinstance(result[key], dict) and isinstance(value, dict):
                result[key] = self._deep_merge(result[key], value)
            else:
                result[key] = value
        return result

    def _setup_logging(self):
        """Setup structured logging for data science workflows"""
        logging.basicConfig(
            level=getattr(logging, self.config.get('log_level', 'INFO').upper()),
            format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
            handlers=[
                logging.StreamHandler(),
                logging.FileHandler('wrfcoin_data_science.log')
            ]
        )
        self.logger = logging.getLogger('WrfcoinDataScience')

    async def initialize(self) -> 'WrfcoinDataScienceManager':
        """Initialize the client with advanced features"""
        self.logger.info("🧪 Initializing WRFCoin Data Science Manager")

        try:
            # Create aiohttp session with custom settings
            connector = aiohttp.TCPConnector(
                limit=self.config['max_concurrent_requests'],
                ttl_dns_cache=300,
                use_dns_cache=True,
                keepalive_timeout=30
            )

            timeout = aiohttp.ClientTimeout(total=self.config['request_timeout'])

            self.session = aiohttp.ClientSession(
                connector=connector,
                timeout=timeout,
                headers={'User-Agent': 'WRFCoin-DataScience/1.0'}
            )

            # Initialize WRFCoin client
            self.client = WrfcoinClient(
                network=self.config['network'],
                api_url=self.config['api_url'],
                api_key=self.config['api_key'],
                session=self.session
            )

            # Validate connection
            await self._validate_connection()

            # Setup rate limiting
            await self._setup_rate_limiting()

            self.logger.info("✅ WRFCoin Data Science Manager initialized")
            return self

        except Exception as e:
            self.logger.error(f"❌ Initialization failed: {e}")
            await self.cleanup()
            raise

    async def _validate_connection(self):
        """Validate connection and measure performance"""
        start_time = time.time()

        try:
            network_info = await self.client.get_network_info()
            response_time = time.time() - start_time

            self.metrics.average_response_time = response_time
            self.metrics.last_request_time = time.time()

            self.logger.info(f"🌐 Connected to {network_info['name']}")
            self.logger.info(f"⚡ Response time: {response_time:.3f}s")
            self.logger.info(f"📊 Block height: {network_info['block_height']}")

            if response_time > 5.0:
                self.logger.warning("⚠️  High latency detected")

        except Exception as e:
            self.logger.error(f"❌ Connection validation failed: {e}")
            raise

    async def _setup_rate_limiting(self):
        """Setup intelligent rate limiting"""
        # Get current rate limits from the API
        try:
            rate_limits = await self.client.get_rate_limits()
            self.logger.info(f"📋 Rate limits: {rate_limits}")

            # Setup adaptive rate limiter based on API response
            self._rate_limiter = AdaptiveRateLimiter(
                requests_per_second=rate_limits.get('requests_per_second', 10),
                burst_capacity=rate_limits.get('burst_capacity', 50)
            )

        except Exception as e:
            self.logger.warning(f"⚠️  Could not fetch rate limits: {e}")
            # Use default conservative rate limiting
            self._rate_limiter = AdaptiveRateLimiter(
                requests_per_second=5,
                burst_capacity=20
            )

    @asynccontextmanager
    async def execute_with_retry(self, operation_name: str):
        """Context manager for operations with retry logic"""
        for attempt in range(self.retry_config.max_retries + 1):
            try:
                start_time = time.time()
                yield

                # Success - update metrics
                duration = time.time() - start_time
                self.metrics.successful_requests += 1
                self.metrics.total_requests += 1

                # Update average response time
                if self.metrics.average_response_time == 0:
                    self.metrics.average_response_time = duration
                else:
                    self.metrics.average_response_time = (
                        self.metrics.average_response_time * 0.9 + duration * 0.1
                    )

                self.logger.debug(f"✅ {operation_name} completed in {duration:.3f}s")
                return

            except (NetworkError, RateLimitError, TimeoutError) as e:
                self.metrics.failed_requests += 1
                self.metrics.total_requests += 1

                if attempt < self.retry_config.max_retries:
                    delay = self._calculate_retry_delay(attempt)
                    self.logger.warning(
                        f"🔄 {operation_name} failed (attempt {attempt + 1}), "
                        f"retrying in {delay:.1f}s: {e}"
                    )
                    await asyncio.sleep(delay)
                else:
                    self.logger.error(f"❌ {operation_name} failed after {attempt + 1} attempts")
                    raise

            except (ValidationError, InsufficientBalanceError) as e:
                # Non-retryable errors
                self.metrics.failed_requests += 1
                self.metrics.total_requests += 1
                self.logger.error(f"❌ {operation_name} failed with non-retryable error: {e}")
                raise

    def _calculate_retry_delay(self, attempt: int) -> float:
        """Calculate exponential backoff delay with jitter"""
        delay = min(
            self.retry_config.base_delay * (self.retry_config.exponential_base ** attempt),
            self.retry_config.max_delay
        )

        if self.retry_config.jitter:
            # Add up to 25% jitter
            jitter = delay * 0.25 * np.random.random()
            delay += jitter

        return delay

    async def cleanup(self):
        """Clean up resources"""
        if self.session:
            await self.session.close()

        self.logger.info("🧹 WRFCoin Data Science Manager cleaned up")

    def get_performance_metrics(self) -> Dict[str, Any]:
        """Get current performance metrics"""
        success_rate = (
            self.metrics.successful_requests / self.metrics.total_requests
            if self.metrics.total_requests > 0 else 0
        )

        return {
            'total_requests': self.metrics.total_requests,
            'successful_requests': self.metrics.successful_requests,
            'failed_requests': self.metrics.failed_requests,
            'success_rate': success_rate,
            'average_response_time': self.metrics.average_response_time,
            'last_request_time': self.metrics.last_request_time
        }

class AdaptiveRateLimiter:
    """Adaptive rate limiter with burst capacity"""

    def __init__(self, requests_per_second: float, burst_capacity: int):
        self.requests_per_second = requests_per_second
        self.burst_capacity = burst_capacity
        self.tokens = burst_capacity
        self.last_refill = time.time()
        self._lock = asyncio.Lock()

    async def acquire(self) -> bool:
        """Acquire a token from the rate limiter"""
        async with self._lock:
            now = time.time()

            # Refill tokens based on time elapsed
            time_elapsed = now - self.last_refill
            self.tokens = min(
                self.burst_capacity,
                self.tokens + time_elapsed * self.requests_per_second
            )
            self.last_refill = now

            if self.tokens >= 1:
                self.tokens -= 1
                return True
            else:
                # Calculate wait time for next token
                wait_time = (1 - self.tokens) / self.requests_per_second
                await asyncio.sleep(wait_time)
                self.tokens = 0
                return True

# Export the manager
wrfcoin_manager = WrfcoinDataScienceManager()
```

### Scientific Data Processing Pipeline

```python
# services/climate_data_processor.py
import asyncio
import io
import gzip
import pickle
from concurrent.futures import ThreadPoolExecutor
from typing import Dict, List, Optional, Union, Any
import pandas as pd
import numpy as np
import xarray as xr
from scipy import stats
from sklearn.preprocessing import StandardScaler
from sklearn.ensemble import IsolationForest
import warnings

class ClimateDataProcessor:
    """Advanced climate data processing for blockchain submission"""

    def __init__(self, wrfcoin_manager, config: Optional[Dict] = None):
        self.wrfcoin = wrfcoin_manager
        self.config = config or {}
        self.thread_pool = ThreadPoolExecutor(
            max_workers=self.config.get('max_workers', 4)
        )

        # Data validation models
        self.anomaly_detector = None
        self.scaler = StandardScaler()

        # Quality control thresholds
        self.quality_thresholds = {
            'temperature': {'min': -100, 'max': 60, 'std_threshold': 3},
            'humidity': {'min': 0, 'max': 100, 'std_threshold': 2.5},
            'pressure': {'min': 800, 'max': 1100, 'std_threshold': 3},
            'wind_speed': {'min': 0, 'max': 150, 'std_threshold': 2.5},
            'precipitation': {'min': 0, 'max': 500, 'std_threshold': 3}
        }

    async def initialize_quality_models(self):
        """Initialize machine learning models for data quality assessment"""
        self.wrfcoin.logger.info("🤖 Initializing quality control models")

        try:
            # Download historical data for model training
            historical_data = await self._fetch_historical_training_data()

            if len(historical_data) > 1000:  # Minimum samples for training
                await self._train_anomaly_detector(historical_data)
                self.wrfcoin.logger.info("✅ Quality control models initialized")
            else:
                self.wrfcoin.logger.warning("⚠️  Insufficient data for model training")

        except Exception as e:
            self.wrfcoin.logger.warning(f"⚠️  Model initialization failed: {e}")

    async def _fetch_historical_training_data(self) -> pd.DataFrame:
        """Fetch historical data for training quality models"""
        async with self.wrfcoin.execute_with_retry("fetch_training_data"):
            # Fetch last 10,000 validated data points
            data = await self.wrfcoin.client.weather.get_historical_data(
                limit=10000,
                validated_only=True,
                format='pandas'
            )
            return data

    async def _train_anomaly_detector(self, data: pd.DataFrame):
        """Train anomaly detection model"""

        def train_model():
            # Prepare features for anomaly detection
            features = ['temperature', 'humidity', 'pressure', 'wind_speed']
            feature_data = data[features].dropna()

            if len(feature_data) < 100:
                return None

            # Scale features
            scaled_features = self.scaler.fit_transform(feature_data)

            # Train isolation forest
            model = IsolationForest(
                contamination=0.1,  # Assume 10% outliers
                random_state=42,
                n_jobs=-1
            )
            model.fit(scaled_features)

            return model

        # Run training in thread pool
        self.anomaly_detector = await asyncio.get_event_loop().run_in_executor(
            self.thread_pool, train_model
        )

    async def process_weather_dataframe(self, df: pd.DataFrame) -> Dict[str, Any]:
        """Process a pandas DataFrame of weather data"""
        self.wrfcoin.logger.info(f"📊 Processing DataFrame with {len(df)} records")

        try:
            # Data validation and cleaning
            cleaned_df = await self._clean_and_validate_dataframe(df)

            # Quality assessment
            quality_report = await self._assess_data_quality(cleaned_df)

            # Aggregate and summarize
            summary_stats = await self._calculate_summary_statistics(cleaned_df)

            # Submit to blockchain in batches
            submission_results = await self._submit_dataframe_to_blockchain(
                cleaned_df, quality_report
            )

            return {
                'original_records': len(df),
                'processed_records': len(cleaned_df),
                'quality_report': quality_report,
                'summary_statistics': summary_stats,
                'submission_results': submission_results
            }

        except Exception as e:
            self.wrfcoin.logger.error(f"❌ DataFrame processing failed: {e}")
            raise

    async def _clean_and_validate_dataframe(self, df: pd.DataFrame) -> pd.DataFrame:
        """Clean and validate weather data"""

        def clean_data():
            cleaned = df.copy()

            # Remove duplicates
            cleaned = cleaned.drop_duplicates()

            # Handle missing values
            for column in cleaned.columns:
                if column in self.quality_thresholds:
                    # Fill missing values with interpolation
                    cleaned[column] = cleaned[column].interpolate(method='linear')

                    # Remove extreme outliers
                    thresholds = self.quality_thresholds[column]
                    cleaned = cleaned[
                        (cleaned[column] >= thresholds['min']) &
                        (cleaned[column] <= thresholds['max'])
                    ]

            # Ensure timestamp column exists and is valid
            if 'timestamp' not in cleaned.columns:
                cleaned['timestamp'] = pd.Timestamp.now()
            else:
                cleaned['timestamp'] = pd.to_datetime(cleaned['timestamp'])
                # Remove future timestamps
                cleaned = cleaned[cleaned['timestamp'] <= pd.Timestamp.now()]

            # Sort by timestamp
            cleaned = cleaned.sort_values('timestamp')

            return cleaned

        # Run cleaning in thread pool for large datasets
        if len(df) > 10000:
            cleaned_df = await asyncio.get_event_loop().run_in_executor(
                self.thread_pool, clean_data
            )
        else:
            cleaned_df = clean_data()

        self.wrfcoin.logger.info(
            f"🧹 Cleaned data: {len(df)} → {len(cleaned_df)} records "
            f"({(len(cleaned_df)/len(df)*100):.1f}% retained)"
        )

        return cleaned_df

    async def _assess_data_quality(self, df: pd.DataFrame) -> Dict[str, Any]:
        """Assess data quality using statistical and ML methods"""

        def calculate_quality():
            quality = {
                'completeness': {},
                'accuracy': {},
                'consistency': {},
                'anomalies': {},
                'overall_score': 0.0
            }

            # Completeness assessment
            for column in df.columns:
                if column in self.quality_thresholds:
                    completeness = 1 - (df[column].isna().sum() / len(df))
                    quality['completeness'][column] = completeness

            # Accuracy assessment (range validation)
            for column, thresholds in self.quality_thresholds.items():
                if column in df.columns:
                    valid_range = (
                        (df[column] >= thresholds['min']) &
                        (df[column] <= thresholds['max'])
                    ).sum() / len(df)
                    quality['accuracy'][column] = valid_range

            # Consistency assessment (statistical outliers)
            for column in df.columns:
                if column in self.quality_thresholds and df[column].dtype in ['float64', 'int64']:
                    z_scores = np.abs(stats.zscore(df[column].dropna()))
                    outlier_ratio = (z_scores > self.quality_thresholds[column]['std_threshold']).sum() / len(df)
                    quality['consistency'][column] = 1 - outlier_ratio

            # Anomaly detection with ML model
            if self.anomaly_detector is not None:
                try:
                    features = ['temperature', 'humidity', 'pressure', 'wind_speed']
                    feature_data = df[features].dropna()

                    if len(feature_data) > 0:
                        scaled_features = self.scaler.transform(feature_data)
                        anomaly_scores = self.anomaly_detector.decision_function(scaled_features)
                        anomaly_ratio = (anomaly_scores < 0).sum() / len(feature_data)
                        quality['anomalies']['ml_detected_ratio'] = anomaly_ratio
                except Exception as e:
                    quality['anomalies']['ml_error'] = str(e)

            # Calculate overall quality score
            all_scores = []
            for category in ['completeness', 'accuracy', 'consistency']:
                if quality[category]:
                    all_scores.extend(quality[category].values())

            if all_scores:
                quality['overall_score'] = np.mean(all_scores)

            return quality

        # Run quality assessment in thread pool for large datasets
        if len(df) > 5000:
            quality_report = await asyncio.get_event_loop().run_in_executor(
                self.thread_pool, calculate_quality
            )
        else:
            quality_report = calculate_quality()

        self.wrfcoin.logger.info(
            f"📈 Data quality score: {quality_report['overall_score']:.3f}"
        )

        return quality_report

    async def _calculate_summary_statistics(self, df: pd.DataFrame) -> Dict[str, Any]:
        """Calculate comprehensive summary statistics"""

        def calculate_stats():
            stats = {}

            for column in df.columns:
                if df[column].dtype in ['float64', 'int64']:
                    col_stats = {
                        'count': int(df[column].count()),
                        'mean': float(df[column].mean()),
                        'std': float(df[column].std()),
                        'min': float(df[column].min()),
                        'max': float(df[column].max()),
                        'q25': float(df[column].quantile(0.25)),
                        'q50': float(df[column].quantile(0.50)),
                        'q75': float(df[column].quantile(0.75))
                    }
                    stats[column] = col_stats

            # Time series analysis
            if 'timestamp' in df.columns:
                time_span = df['timestamp'].max() - df['timestamp'].min()
                stats['temporal'] = {
                    'start_time': df['timestamp'].min().isoformat(),
                    'end_time': df['timestamp'].max().isoformat(),
                    'duration_hours': time_span.total_seconds() / 3600,
                    'sampling_frequency': len(df) / (time_span.total_seconds() / 3600) if time_span.total_seconds() > 0 else 0
                }

            return stats

        # Run statistics calculation in thread pool
        summary_stats = await asyncio.get_event_loop().run_in_executor(
            self.thread_pool, calculate_stats
        )

        return summary_stats

    async def _submit_dataframe_to_blockchain(
        self,
        df: pd.DataFrame,
        quality_report: Dict[str, Any]
    ) -> Dict[str, Any]:
        """Submit processed DataFrame to blockchain in optimized batches"""

        chunk_size = self.config.get('chunk_size', 100)
        max_concurrent = self.config.get('max_concurrent_submissions', 5)

        # Split DataFrame into chunks
        chunks = [df[i:i+chunk_size] for i in range(0, len(df), chunk_size)]

        self.wrfcoin.logger.info(
            f"📤 Submitting {len(df)} records in {len(chunks)} chunks "
            f"(max {max_concurrent} concurrent)"
        )

        submission_results = {
            'total_chunks': len(chunks),
            'successful_chunks': 0,
            'failed_chunks': 0,
            'total_records_submitted': 0,
            'total_rewards_earned': 0.0,
            'chunk_results': []
        }

        # Process chunks with controlled concurrency
        semaphore = asyncio.Semaphore(max_concurrent)

        async def submit_chunk(chunk_index: int, chunk_df: pd.DataFrame):
            async with semaphore:
                try:
                    async with self.wrfcoin.execute_with_retry(f"submit_chunk_{chunk_index}"):
                        # Convert chunk to format expected by API
                        chunk_data = self._dataframe_to_api_format(chunk_df, quality_report)

                        # Submit to blockchain
                        result = await self.wrfcoin.client.weather.submit_batch(chunk_data)

                        submission_results['successful_chunks'] += 1
                        submission_results['total_records_submitted'] += len(chunk_df)
                        submission_results['total_rewards_earned'] += result.get('total_reward', 0)

                        chunk_result = {
                            'chunk_index': chunk_index,
                            'records': len(chunk_df),
                            'status': 'success',
                            'reward': result.get('total_reward', 0),
                            'submission_id': result.get('submission_id')
                        }
                        submission_results['chunk_results'].append(chunk_result)

                        self.wrfcoin.logger.debug(
                            f"✅ Chunk {chunk_index} submitted: {len(chunk_df)} records, "
                            f"{result.get('total_reward', 0)} WRF earned"
                        )

                except Exception as e:
                    submission_results['failed_chunks'] += 1

                    chunk_result = {
                        'chunk_index': chunk_index,
                        'records': len(chunk_df),
                        'status': 'failed',
                        'error': str(e)
                    }
                    submission_results['chunk_results'].append(chunk_result)

                    self.wrfcoin.logger.error(
                        f"❌ Chunk {chunk_index} submission failed: {e}"
                    )

        # Submit all chunks concurrently
        tasks = [
            submit_chunk(i, chunk)
            for i, chunk in enumerate(chunks)
        ]

        await asyncio.gather(*tasks, return_exceptions=True)

        success_rate = submission_results['successful_chunks'] / submission_results['total_chunks']

        self.wrfcoin.logger.info(
            f"📊 Submission complete: {submission_results['successful_chunks']}/{submission_results['total_chunks']} "
            f"chunks successful ({success_rate:.1%}), "
            f"{submission_results['total_records_submitted']} records submitted, "
            f"{submission_results['total_rewards_earned']:.3f} WRF earned"
        )

        return submission_results

    def _dataframe_to_api_format(self, df: pd.DataFrame, quality_report: Dict[str, Any]) -> List[Dict[str, Any]]:
        """Convert DataFrame to API-compatible format"""

        records = []
        for _, row in df.iterrows():
            record = {
                'timestamp': row.get('timestamp', pd.Timestamp.now()).isoformat(),
                'location': {
                    'latitude': row.get('latitude', 0.0),
                    'longitude': row.get('longitude', 0.0),
                    'elevation': row.get('elevation', 0.0)
                },
                'measurements': {},
                'quality_score': quality_report.get('overall_score', 0.5),
                'source': row.get('source', 'data_science_pipeline'),
                'metadata': {
                    'processing_version': '1.0',
                    'quality_checks_passed': quality_report.get('overall_score', 0) > 0.7
                }
            }

            # Add measurement data
            for column, value in row.items():
                if column in self.quality_thresholds and pd.notna(value):
                    record['measurements'][column] = float(value)

            records.append(record)

        return records

# Export the processor
climate_processor = ClimateDataProcessor
```

---

## 2. Scientific Data Workflows

### Automated Climate Station Integration

```python
# examples/automated_climate_station.py
import asyncio
import pandas as pd
import numpy as np
from datetime import datetime, timedelta
import aiofiles
import schedule
import time
from pathlib import Path

class AutomatedClimateStation:
    """Automated climate data collection and submission system"""

    def __init__(self, station_config: Dict[str, Any]):
        self.config = station_config
        self.station_id = station_config['station_id']
        self.location = station_config['location']
        self.sensors = station_config['sensors']

        # Initialize components
        self.wrfcoin = None
        self.processor = None
        self.data_buffer = []
        self.last_submission = None

        # Scheduling
        self.submission_interval = station_config.get('submission_interval_minutes', 15)
        self.quality_check_interval = station_config.get('quality_check_interval_hours', 6)

    async def initialize(self):
        """Initialize the automated station"""
        print(f"🌡️  Initializing Climate Station: {self.station_id}")

        # Initialize WRFCoin manager
        from config.wrfcoin_manager import wrfcoin_manager
        self.wrfcoin = await wrfcoin_manager.initialize()

        # Initialize data processor
        from services.climate_data_processor import climate_processor
        self.processor = climate_processor(self.wrfcoin)
        await self.processor.initialize_quality_models()

        # Register station with blockchain
        await self._register_station()

        # Setup data collection scheduling
        self._setup_scheduling()

        print(f"✅ Climate Station {self.station_id} initialized successfully")

    async def _register_station(self):
        """Register this station with the WRFCoin network"""
        try:
            registration_data = {
                'station_id': self.station_id,
                'name': self.config.get('name', f'Climate Station {self.station_id}'),
                'location': self.location,
                'sensors': self.sensors,
                'operator': self.config.get('operator', 'Automated System'),
                'calibration_date': self.config.get('calibration_date'),
                'certification_level': self.config.get('certification_level', 'standard')
            }

            result = await self.wrfcoin.client.stations.register(registration_data)
            print(f"📝 Station registered: {result['registration_id']}")

        except Exception as e:
            print(f"⚠️  Station registration failed: {e}")

    def _setup_scheduling(self):
        """Setup automated data collection and submission"""
        # Schedule regular data submission
        schedule.every(self.submission_interval).minutes.do(
            lambda: asyncio.create_task(self._submit_buffered_data())
        )

        # Schedule quality checks
        schedule.every(self.quality_check_interval).hours.do(
            lambda: asyncio.create_task(self._perform_quality_check())
        )

        # Schedule daily maintenance
        schedule.every().day.at("02:00").do(
            lambda: asyncio.create_task(self._daily_maintenance())
        )

    async def collect_sensor_data(self) -> Dict[str, Any]:
        """Collect data from all configured sensors"""
        timestamp = datetime.now()
        measurements = {}

        try:
            # Simulate or collect real sensor data
            for sensor_type in self.sensors:
                if sensor_type == 'temperature':
                    measurements['temperature'] = await self._read_temperature_sensor()
                elif sensor_type == 'humidity':
                    measurements['humidity'] = await self._read_humidity_sensor()
                elif sensor_type == 'pressure':
                    measurements['pressure'] = await self._read_pressure_sensor()
                elif sensor_type == 'wind_speed':
                    measurements['wind_speed'] = await self._read_wind_sensor()
                elif sensor_type == 'precipitation':
                    measurements['precipitation'] = await self._read_precipitation_sensor()
                elif sensor_type == 'air_quality':
                    measurements.update(await self._read_air_quality_sensors())

            # Add metadata
            data_point = {
                'timestamp': timestamp,
                'station_id': self.station_id,
                'latitude': self.location['latitude'],
                'longitude': self.location['longitude'],
                'elevation': self.location.get('elevation', 0),
                **measurements
            }

            # Add to buffer
            self.data_buffer.append(data_point)

            print(f"📊 Data collected: {len(measurements)} measurements")
            return data_point

        except Exception as e:
            print(f"❌ Sensor data collection failed: {e}")
            return None

    async def _read_temperature_sensor(self) -> float:
        """Read temperature sensor (replace with actual sensor code)"""
        # Simulate realistic temperature data with some noise
        base_temp = 20 + 10 * np.sin(time.time() / 86400 * 2 * np.pi)  # Daily cycle
        noise = np.random.normal(0, 0.5)
        return round(base_temp + noise, 1)

    async def _read_humidity_sensor(self) -> float:
        """Read humidity sensor"""
        # Simulate humidity inversely related to temperature
        base_humidity = 60 - 2 * np.sin(time.time() / 86400 * 2 * np.pi)
        noise = np.random.normal(0, 2)
        return max(0, min(100, round(base_humidity + noise, 1)))

    async def _read_pressure_sensor(self) -> float:
        """Read atmospheric pressure sensor"""
        # Simulate pressure with weather patterns
        base_pressure = 1013.25 + 10 * np.sin(time.time() / 259200 * 2 * np.pi)  # 3-day cycle
        noise = np.random.normal(0, 1)
        return round(base_pressure + noise, 2)

    async def _read_wind_sensor(self) -> float:
        """Read wind speed sensor"""
        # Simulate wind with random gusts
        base_wind = 5 + 3 * np.random.exponential(1)
        return round(min(base_wind, 25), 1)

    async def _read_precipitation_sensor(self) -> float:
        """Read precipitation sensor"""
        # Simulate occasional precipitation
        if np.random.random() < 0.05:  # 5% chance of precipitation
            return round(np.random.exponential(2), 1)
        return 0.0

    async def _read_air_quality_sensors(self) -> Dict[str, float]:
        """Read air quality sensors"""
        return {
            'pm25': round(np.random.normal(15, 5), 1),
            'pm10': round(np.random.normal(25, 8), 1),
            'co2': round(np.random.normal(410, 20), 1),
            'ozone': round(np.random.normal(50, 10), 1)
        }

    async def _submit_buffered_data(self):
        """Submit buffered data to blockchain"""
        if not self.data_buffer:
            print("📭 No data to submit")
            return

        try:
            print(f"📤 Submitting {len(self.data_buffer)} data points...")

            # Convert buffer to DataFrame
            df = pd.DataFrame(self.data_buffer)

            # Process and submit
            result = await self.processor.process_weather_dataframe(df)

            # Update metrics
            self.last_submission = datetime.now()

            # Clear buffer on success
            self.data_buffer = []

            print(f"✅ Submission successful:")
            print(f"   - Records submitted: {result['submission_results']['total_records_submitted']}")
            print(f"   - Rewards earned: {result['submission_results']['total_rewards_earned']:.3f} WRF")
            print(f"   - Quality score: {result['quality_report']['overall_score']:.3f}")

        except Exception as e:
            print(f"❌ Data submission failed: {e}")
            # Keep data in buffer for retry

    async def _perform_quality_check(self):
        """Perform automated quality checks"""
        print("🔍 Performing quality check...")

        try:
            # Check sensor health
            sensor_health = await self._check_sensor_health()

            # Check recent data quality
            if len(self.data_buffer) > 10:
                df = pd.DataFrame(self.data_buffer[-10:])  # Last 10 readings
                quality_report = await self.processor._assess_data_quality(df)

                if quality_report['overall_score'] < 0.5:
                    print(f"⚠️  Low data quality detected: {quality_report['overall_score']:.3f}")
                    await self._trigger_maintenance_alert()
                else:
                    print(f"✅ Data quality good: {quality_report['overall_score']:.3f}")

            # Check connectivity
            network_status = await self.wrfcoin.client.get_network_info()
            print(f"📡 Network status: {network_status['status']}")

        except Exception as e:
            print(f"❌ Quality check failed: {e}")

    async def _check_sensor_health(self) -> Dict[str, Any]:
        """Check health status of all sensors"""
        health_status = {}

        for sensor_type in self.sensors:
            try:
                # Simulate sensor health check
                # In real implementation, this would check sensor responsiveness,
                # calibration status, error rates, etc.

                health_status[sensor_type] = {
                    'status': 'healthy',
                    'last_calibration': '2024-01-01',
                    'error_rate': np.random.random() * 0.01,  # < 1% error rate
                    'response_time': np.random.random() * 100  # < 100ms
                }

            except Exception as e:
                health_status[sensor_type] = {
                    'status': 'error',
                    'error': str(e)
                }

        return health_status

    async def _trigger_maintenance_alert(self):
        """Trigger maintenance alert for station issues"""
        alert_data = {
            'station_id': self.station_id,
            'alert_type': 'maintenance_required',
            'timestamp': datetime.now().isoformat(),
            'location': self.location,
            'description': 'Automated quality check detected issues requiring attention'
        }

        try:
            # Submit alert to blockchain for transparency
            await self.wrfcoin.client.stations.submit_alert(alert_data)
            print("🚨 Maintenance alert submitted to blockchain")

        except Exception as e:
            print(f"❌ Failed to submit maintenance alert: {e}")

    async def _daily_maintenance(self):
        """Perform daily maintenance tasks"""
        print("🔧 Performing daily maintenance...")

        try:
            # Archive old data
            await self._archive_old_data()

            # Update performance metrics
            await self._update_performance_metrics()

            # Check for software updates
            await self._check_for_updates()

            # Cleanup temporary files
            await self._cleanup_temp_files()

            print("✅ Daily maintenance completed")

        except Exception as e:
            print(f"❌ Daily maintenance failed: {e}")

    async def _archive_old_data(self):
        """Archive data older than specified retention period"""
        retention_days = self.config.get('data_retention_days', 30)
        cutoff_date = datetime.now() - timedelta(days=retention_days)

        # Implementation would move old data to archive storage
        print(f"📁 Archiving data older than {retention_days} days")

    async def _update_performance_metrics(self):
        """Update station performance metrics"""
        metrics = self.wrfcoin.get_performance_metrics()

        # Store metrics for monitoring
        metrics_data = {
            'station_id': self.station_id,
            'date': datetime.now().date().isoformat(),
            'uptime': self._calculate_uptime(),
            'data_quality_avg': 0.85,  # Would calculate from recent submissions
            'submission_success_rate': metrics['success_rate'],
            'total_rewards_earned': 0.0  # Would track cumulative rewards
        }

        # Save metrics locally and optionally submit to blockchain
        await self._save_metrics(metrics_data)

    def _calculate_uptime(self) -> float:
        """Calculate station uptime percentage"""
        # Implementation would track actual uptime
        return 99.5  # 99.5% uptime

    async def _save_metrics(self, metrics_data: Dict[str, Any]):
        """Save performance metrics"""
        metrics_file = Path(f"data/metrics/{self.station_id}_metrics.json")
        metrics_file.parent.mkdir(parents=True, exist_ok=True)

        # Append to metrics file
        async with aiofiles.open(metrics_file, 'a') as f:
            await f.write(f"{metrics_data}\n")

    async def _check_for_updates(self):
        """Check for software/firmware updates"""
        # Implementation would check for updates
        print("🔄 Checking for updates...")

    async def _cleanup_temp_files(self):
        """Clean up temporary files"""
        # Implementation would clean temporary files
        print("🧹 Cleaning temporary files...")

    async def run_continuously(self):
        """Run the automated station continuously"""
        print(f"🚀 Starting continuous operation for station {self.station_id}")

        try:
            while True:
                # Collect sensor data
                await self.collect_sensor_data()

                # Run scheduled tasks
                schedule.run_pending()

                # Wait before next collection
                collection_interval = self.config.get('collection_interval_seconds', 60)
                await asyncio.sleep(collection_interval)

        except KeyboardInterrupt:
            print("\n🛑 Shutting down climate station...")
            await self.shutdown()
        except Exception as e:
            print(f"❌ Station operation error: {e}")
            raise

    async def shutdown(self):
        """Gracefully shutdown the station"""
        # Submit any remaining buffered data
        if self.data_buffer:
            await self._submit_buffered_data()

        # Cleanup resources
        if self.wrfcoin:
            await self.wrfcoin.cleanup()

        print(f"✅ Climate Station {self.station_id} shutdown complete")

# Example usage
async def main():
    station_config = {
        'station_id': 'CLIMATE_STATION_001',
        'name': 'Research Campus Weather Station',
        'location': {
            'latitude': 37.7749,
            'longitude': -122.4194,
            'elevation': 16
        },
        'sensors': ['temperature', 'humidity', 'pressure', 'wind_speed', 'precipitation', 'air_quality'],
        'operator': 'University Research Center',
        'submission_interval_minutes': 15,
        'collection_interval_seconds': 60,
        'quality_check_interval_hours': 6,
        'data_retention_days': 90
    }

    station = AutomatedClimateStation(station_config)
    await station.initialize()
    await station.run_continuously()

if __name__ == "__main__":
    asyncio.run(main())
```

---

## 3. Machine Learning Integration

### Predictive Climate Modeling with Blockchain Rewards

```python
# services/ml_climate_predictor.py
import asyncio
import joblib
import numpy as np
import pandas as pd
from datetime import datetime, timedelta
from typing import Dict, List, Optional, Tuple, Any
from sklearn.ensemble import RandomForestRegressor, GradientBoostingRegressor
from sklearn.model_selection import train_test_split, cross_val_score
from sklearn.metrics import mean_squared_error, r2_score
from sklearn.preprocessing import StandardScaler
import xgboost as xgb
import tensorflow as tf
from tensorflow import keras

class MLClimatePredictor:
    """Machine learning system for climate prediction with blockchain integration"""

    def __init__(self, wrfcoin_manager, config: Optional[Dict] = None):
        self.wrfcoin = wrfcoin_manager
        self.config = config or {}

        # Model configurations
        self.models = {
            'temperature': None,
            'humidity': None,
            'pressure': None,
            'precipitation': None
        }

        self.scalers = {
            'temperature': StandardScaler(),
            'humidity': StandardScaler(),
            'pressure': StandardScaler(),
            'precipitation': StandardScaler()
        }

        # Prediction accuracy tracking
        self.prediction_history = []
        self.accuracy_metrics = {}

        # Blockchain reward thresholds
        self.accuracy_thresholds = {
            'excellent': 0.95,  # > 95% accuracy
            'good': 0.85,       # > 85% accuracy
            'acceptable': 0.75  # > 75% accuracy
        }

    async def initialize_models(self):
        """Initialize and train ML models"""
        self.wrfcoin.logger.info("🤖 Initializing ML climate prediction models")

        try:
            # Fetch training data
            training_data = await self._fetch_training_data()

            if len(training_data) < 1000:
                raise ValueError("Insufficient training data")

            # Train models for each climate variable
            for variable in self.models.keys():
                await self._train_model(variable, training_data)

            # Validate models
            validation_results = await self._validate_models(training_data)

            self.wrfcoin.logger.info("✅ ML models initialized successfully")
            self.wrfcoin.logger.info(f"📊 Validation results: {validation_results}")

            return validation_results

        except Exception as e:
            self.wrfcoin.logger.error(f"❌ Model initialization failed: {e}")
            raise

    async def _fetch_training_data(self) -> pd.DataFrame:
        """Fetch historical data for model training"""
        async with self.wrfcoin.execute_with_retry("fetch_ml_training_data"):
            # Fetch last 50,000 validated data points
            data = await self.wrfcoin.client.weather.get_historical_data(
                limit=50000,
                validated_only=True,
                include_location=True,
                include_metadata=True,
                format='pandas'
            )

            # Feature engineering
            data = self._engineer_features(data)

            self.wrfcoin.logger.info(f"📚 Training data fetched: {len(data)} samples")
            return data

    def _engineer_features(self, data: pd.DataFrame) -> pd.DataFrame:
        """Engineer features for better model performance"""
        df = data.copy()

        # Temporal features
        df['timestamp'] = pd.to_datetime(df['timestamp'])
        df['hour'] = df['timestamp'].dt.hour
        df['day_of_year'] = df['timestamp'].dt.dayofyear
        df['month'] = df['timestamp'].dt.month
        df['season'] = df['month'].apply(self._get_season)

        # Cyclical encoding for temporal features
        df['hour_sin'] = np.sin(2 * np.pi * df['hour'] / 24)
        df['hour_cos'] = np.cos(2 * np.pi * df['hour'] / 24)
        df['day_sin'] = np.sin(2 * np.pi * df['day_of_year'] / 365)
        df['day_cos'] = np.cos(2 * np.pi * df['day_of_year'] / 365)

        # Geographical features
        df['lat_rad'] = np.radians(df['latitude'])
        df['lng_rad'] = np.radians(df['longitude'])

        # Derived meteorological features
        if 'temperature' in df.columns and 'humidity' in df.columns:
            df['heat_index'] = self._calculate_heat_index(df['temperature'], df['humidity'])
            df['dew_point'] = self._calculate_dew_point(df['temperature'], df['humidity'])

        # Lagged features (previous measurements)
        for var in ['temperature', 'humidity', 'pressure']:
            if var in df.columns:
                df[f'{var}_lag1'] = df[var].shift(1)
                df[f'{var}_lag24'] = df[var].shift(24)  # 24 hours ago

        # Rolling statistics
        for var in ['temperature', 'humidity', 'pressure']:
            if var in df.columns:
                df[f'{var}_rolling_mean_6h'] = df[var].rolling(window=6, min_periods=1).mean()
                df[f'{var}_rolling_std_6h'] = df[var].rolling(window=6, min_periods=1).std()

        return df.dropna()

    def _get_season(self, month: int) -> int:
        """Get season from month (0=Winter, 1=Spring, 2=Summer, 3=Fall)"""
        if month in [12, 1, 2]:
            return 0  # Winter
        elif month in [3, 4, 5]:
            return 1  # Spring
        elif month in [6, 7, 8]:
            return 2  # Summer
        else:
            return 3  # Fall

    def _calculate_heat_index(self, temp: pd.Series, humidity: pd.Series) -> pd.Series:
        """Calculate heat index from temperature and humidity"""
        # Simplified heat index calculation
        hi = temp + 0.5 * (temp - 14.4) * (humidity / 100)
        return hi

    def _calculate_dew_point(self, temp: pd.Series, humidity: pd.Series) -> pd.Series:
        """Calculate dew point from temperature and humidity"""
        # Magnus formula approximation
        a, b = 17.27, 237.7
        alpha = ((a * temp) / (b + temp)) + np.log(humidity / 100)
        dew_point = (b * alpha) / (a - alpha)
        return dew_point

    async def _train_model(self, variable: str, data: pd.DataFrame):
        """Train ML model for specific climate variable"""
        if variable not in data.columns:
            self.wrfcoin.logger.warning(f"⚠️  Variable {variable} not found in training data")
            return

        self.wrfcoin.logger.info(f"🎯 Training model for {variable}")

        # Prepare features and target
        feature_columns = [
            'latitude', 'longitude', 'elevation',
            'hour_sin', 'hour_cos', 'day_sin', 'day_cos',
            'season', 'lat_rad', 'lng_rad'
        ]

        # Add lagged features if available
        lag_features = [col for col in data.columns if 'lag' in col or 'rolling' in col]
        feature_columns.extend(lag_features)

        # Add other climate variables as features (exclude target)
        other_vars = [var for var in ['temperature', 'humidity', 'pressure'] if var != variable]
        feature_columns.extend([var for var in other_vars if var in data.columns])

        # Prepare data
        X = data[feature_columns].fillna(0)
        y = data[variable]

        # Split data
        X_train, X_test, y_train, y_test = train_test_split(
            X, y, test_size=0.2, random_state=42, shuffle=False
        )

        # Scale features
        X_train_scaled = self.scalers[variable].fit_transform(X_train)
        X_test_scaled = self.scalers[variable].transform(X_test)

        # Train ensemble of models
        models = {
            'random_forest': RandomForestRegressor(
                n_estimators=100,
                max_depth=20,
                random_state=42,
                n_jobs=-1
            ),
            'gradient_boosting': GradientBoostingRegressor(
                n_estimators=100,
                max_depth=8,
                learning_rate=0.1,
                random_state=42
            ),
            'xgboost': xgb.XGBRegressor(
                n_estimators=100,
                max_depth=8,
                learning_rate=0.1,
                random_state=42
            )
        }

        # Train and evaluate each model
        model_scores = {}
        trained_models = {}

        for model_name, model in models.items():
            try:
                # Train model
                model.fit(X_train_scaled, y_train)

                # Predict and evaluate
                y_pred = model.predict(X_test_scaled)
                score = r2_score(y_test, y_pred)
                rmse = np.sqrt(mean_squared_error(y_test, y_pred))

                model_scores[model_name] = {'r2': score, 'rmse': rmse}
                trained_models[model_name] = model

                self.wrfcoin.logger.info(
                    f"   {model_name}: R² = {score:.3f}, RMSE = {rmse:.3f}"
                )

            except Exception as e:
                self.wrfcoin.logger.warning(f"   {model_name} training failed: {e}")

        # Select best model
        if model_scores:
            best_model_name = max(model_scores.keys(), key=lambda k: model_scores[k]['r2'])
            self.models[variable] = {
                'model': trained_models[best_model_name],
                'model_type': best_model_name,
                'performance': model_scores[best_model_name],
                'feature_columns': feature_columns,
                'trained_at': datetime.now()
            }

            self.wrfcoin.logger.info(
                f"✅ Best model for {variable}: {best_model_name} "
                f"(R² = {model_scores[best_model_name]['r2']:.3f})"
            )
        else:
            raise ValueError(f"No models successfully trained for {variable}")

    async def _validate_models(self, data: pd.DataFrame) -> Dict[str, Any]:
        """Validate trained models with cross-validation"""
        validation_results = {}

        for variable, model_info in self.models.items():
            if model_info is None:
                continue

            try:
                # Prepare data
                X = data[model_info['feature_columns']].fillna(0)
                y = data[variable]

                # Scale features
                X_scaled = self.scalers[variable].transform(X)

                # Cross-validation
                cv_scores = cross_val_score(
                    model_info['model'], X_scaled, y,
                    cv=5, scoring='r2', n_jobs=-1
                )

                validation_results[variable] = {
                    'cv_mean': cv_scores.mean(),
                    'cv_std': cv_scores.std(),
                    'training_r2': model_info['performance']['r2'],
                    'training_rmse': model_info['performance']['rmse']
                }

            except Exception as e:
                self.wrfcoin.logger.error(f"Validation failed for {variable}: {e}")
                validation_results[variable] = {'error': str(e)}

        return validation_results

    async def make_predictions(
        self,
        locations: List[Dict[str, float]],
        forecast_hours: int = 24
    ) -> Dict[str, Any]:
        """Make climate predictions for specified locations"""

        predictions = {
            'locations': locations,
            'forecast_hours': forecast_hours,
            'predictions': {},
            'confidence_intervals': {},
            'model_metadata': {}
        }

        current_time = datetime.now()

        for i, location in enumerate(locations):
            location_predictions = {}

            for variable, model_info in self.models.items():
                if model_info is None:
                    continue

                try:
                    # Generate time series for forecast period
                    future_times = [
                        current_time + timedelta(hours=h)
                        for h in range(1, forecast_hours + 1)
                    ]

                    # Create feature matrix
                    features_list = []
                    for future_time in future_times:
                        features = self._create_prediction_features(location, future_time)
                        features_list.append(features)

                    features_df = pd.DataFrame(features_list)

                    # Ensure all required columns are present
                    for col in model_info['feature_columns']:
                        if col not in features_df.columns:
                            features_df[col] = 0.0

                    # Select and scale features
                    X = features_df[model_info['feature_columns']].fillna(0)
                    X_scaled = self.scalers[variable].transform(X)

                    # Make predictions
                    pred_values = model_info['model'].predict(X_scaled)

                    location_predictions[variable] = {
                        'values': pred_values.tolist(),
                        'timestamps': [t.isoformat() for t in future_times],
                        'model_type': model_info['model_type'],
                        'model_performance': model_info['performance']
                    }

                except Exception as e:
                    self.wrfcoin.logger.error(f"Prediction failed for {variable}: {e}")
                    location_predictions[variable] = {'error': str(e)}

            predictions['predictions'][f'location_{i}'] = location_predictions

        # Submit predictions to blockchain for validation and rewards
        submission_result = await self._submit_predictions_to_blockchain(predictions)
        predictions['blockchain_submission'] = submission_result

        return predictions

    def _create_prediction_features(self, location: Dict[str, float], timestamp: datetime) -> Dict[str, float]:
        """Create feature vector for prediction"""
        features = {
            'latitude': location['latitude'],
            'longitude': location['longitude'],
            'elevation': location.get('elevation', 0),
            'hour': timestamp.hour,
            'day_of_year': timestamp.timetuple().tm_yday,
            'month': timestamp.month,
            'season': self._get_season(timestamp.month)
        }

        # Cyclical encoding
        features['hour_sin'] = np.sin(2 * np.pi * features['hour'] / 24)
        features['hour_cos'] = np.cos(2 * np.pi * features['hour'] / 24)
        features['day_sin'] = np.sin(2 * np.pi * features['day_of_year'] / 365)
        features['day_cos'] = np.cos(2 * np.pi * features['day_of_year'] / 365)

        # Geographical features
        features['lat_rad'] = np.radians(features['latitude'])
        features['lng_rad'] = np.radians(features['longitude'])

        return features

    async def _submit_predictions_to_blockchain(self, predictions: Dict[str, Any]) -> Dict[str, Any]:
        """Submit predictions to blockchain for validation and rewards"""
        try:
            async with self.wrfcoin.execute_with_retry("submit_predictions"):
                # Format predictions for blockchain submission
                submission_data = {
                    'prediction_type': 'climate_forecast',
                    'model_versions': {
                        var: info.get('model_type', 'unknown')
                        for var, info in self.models.items()
                        if info is not None
                    },
                    'predictions': predictions['predictions'],
                    'forecast_period_hours': predictions['forecast_hours'],
                    'submission_timestamp': datetime.now().isoformat(),
                    'predictor_id': 'ml_climate_predictor_v1'
                }

                result = await self.wrfcoin.client.predictions.submit(submission_data)

                self.wrfcoin.logger.info(f"📤 Predictions submitted: {result['prediction_id']}")

                return {
                    'prediction_id': result['prediction_id'],
                    'status': result['status'],
                    'validation_window_hours': result.get('validation_window_hours', 24),
                    'potential_reward': result.get('potential_reward', 0.0)
                }

        except Exception as e:
            self.wrfcoin.logger.error(f"❌ Prediction submission failed: {e}")
            return {'error': str(e)}

    async def validate_predictions(self, prediction_id: str) -> Dict[str, Any]:
        """Validate previous predictions and claim rewards"""
        try:
            async with self.wrfcoin.execute_with_retry("validate_predictions"):
                # Get prediction validation results
                validation = await self.wrfcoin.client.predictions.get_validation(prediction_id)

                # Calculate accuracy metrics
                accuracy_results = {}
                total_reward = 0.0

                for location, predictions in validation['predictions'].items():
                    location_accuracy = {}

                    for variable, pred_data in predictions.items():
                        if 'actual_values' in pred_data and 'predicted_values' in pred_data:
                            actual = np.array(pred_data['actual_values'])
                            predicted = np.array(pred_data['predicted_values'])

                            # Calculate accuracy metrics
                            mae = np.mean(np.abs(actual - predicted))
                            rmse = np.sqrt(np.mean((actual - predicted) ** 2))

                            # Normalized accuracy (0-1 scale)
                            accuracy = max(0, 1 - rmse / np.std(actual))

                            location_accuracy[variable] = {
                                'accuracy': accuracy,
                                'mae': mae,
                                'rmse': rmse
                            }

                            # Calculate reward based on accuracy
                            reward = self._calculate_prediction_reward(accuracy, variable)
                            total_reward += reward

                            location_accuracy[variable]['reward'] = reward

                    accuracy_results[location] = location_accuracy

                # Update accuracy metrics
                self.accuracy_metrics[prediction_id] = accuracy_results

                # Claim rewards
                if total_reward > 0:
                    claim_result = await self.wrfcoin.client.predictions.claim_reward(
                        prediction_id, total_reward
                    )

                    self.wrfcoin.logger.info(
                        f"💰 Prediction rewards claimed: {total_reward:.3f} WRF"
                    )

                return {
                    'prediction_id': prediction_id,
                    'accuracy_results': accuracy_results,
                    'total_reward': total_reward,
                    'validation_status': validation['status']
                }

        except Exception as e:
            self.wrfcoin.logger.error(f"❌ Prediction validation failed: {e}")
            return {'error': str(e)}

    def _calculate_prediction_reward(self, accuracy: float, variable: str) -> float:
        """Calculate reward based on prediction accuracy"""
        base_reward = 0.1  # Base reward per prediction

        # Accuracy multipliers
        if accuracy >= self.accuracy_thresholds['excellent']:
            multiplier = 5.0
        elif accuracy >= self.accuracy_thresholds['good']:
            multiplier = 3.0
        elif accuracy >= self.accuracy_thresholds['acceptable']:
            multiplier = 1.5
        else:
            multiplier = 0.5

        # Variable-specific multipliers
        variable_multipliers = {
            'temperature': 1.0,
            'humidity': 0.8,
            'pressure': 0.9,
            'precipitation': 1.5  # Higher reward for harder-to-predict variable
        }

        variable_mult = variable_multipliers.get(variable, 1.0)

        return base_reward * multiplier * variable_mult

    async def get_model_performance(self) -> Dict[str, Any]:
        """Get comprehensive model performance metrics"""
        performance = {
            'models': {},
            'prediction_history': self.prediction_history[-100:],  # Last 100
            'accuracy_trends': {},
            'total_rewards_earned': 0.0
        }

        # Model information
        for variable, model_info in self.models.items():
            if model_info is not None:
                performance['models'][variable] = {
                    'model_type': model_info['model_type'],
                    'training_performance': model_info['performance'],
                    'trained_at': model_info['trained_at'].isoformat(),
                    'feature_count': len(model_info['feature_columns'])
                }

        # Calculate accuracy trends
        for prediction_id, accuracy_data in self.accuracy_metrics.items():
            for location, variables in accuracy_data.items():
                for variable, metrics in variables.items():
                    if variable not in performance['accuracy_trends']:
                        performance['accuracy_trends'][variable] = []

                    performance['accuracy_trends'][variable].append({
                        'prediction_id': prediction_id,
                        'accuracy': metrics['accuracy'],
                        'reward': metrics.get('reward', 0.0)
                    })

                    performance['total_rewards_earned'] += metrics.get('reward', 0.0)

        return performance

    async def retrain_models(self) -> Dict[str, Any]:
        """Retrain models with latest data"""
        self.wrfcoin.logger.info("🔄 Retraining ML models with latest data")

        # Backup current models
        model_backup = self.models.copy()

        try:
            # Re-initialize models with fresh data
            validation_results = await self.initialize_models()

            self.wrfcoin.logger.info("✅ Model retraining completed successfully")
            return {
                'status': 'success',
                'validation_results': validation_results,
                'retrained_at': datetime.now().isoformat()
            }

        except Exception as e:
            # Restore backup models on failure
            self.models = model_backup
            self.wrfcoin.logger.error(f"❌ Model retraining failed: {e}")

            return {
                'status': 'failed',
                'error': str(e),
                'models_restored': True
            }

# Export the predictor
ml_predictor = MLClimatePredictor
```

---

## What You've Learned

✅ **Advanced Python Configuration** - Production-ready setup with connection pooling and metrics  
✅ **Scientific Data Processing** - Pandas/NumPy integration with blockchain submission  
✅ **Automated Climate Stations** - IoT integration with quality control and scheduling  
✅ **Machine Learning Integration** - Climate prediction models with blockchain rewards  
✅ **Data Science Workflows** - Large-scale processing with async operations  
✅ **Quality Control Systems** - Automated validation and anomaly detection

## Next Steps

1. **[Smart Contract Integration](../smart-contracts/)** - Deploy and interact with contracts
2. **[Real-World Applications](../real-world-apps/)** - Build complete applications
3. **[Performance Optimization](../optimization/)** - Scale for production workloads
4. **[Research Integration](../../expert/research-integration/)** - Academic workflows

---

**🎉 Congratulations!** You've mastered advanced Python patterns for climate data science on
WRFCoin. You're ready to build sophisticated research applications that contribute to climate
science!
