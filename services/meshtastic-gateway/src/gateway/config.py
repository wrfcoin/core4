"""
Configuration management for Meshtastic Gateway Service.

Uses Pydantic Settings for environment variable support with validation.
"""

from typing import Literal

from pydantic import model_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class GatewaySettings(BaseSettings):
    """Gateway service configuration with environment variable support."""

    model_config = SettingsConfigDict(
        env_prefix="MESHTASTIC_GW_",
        env_file=".env",
        env_file_encoding="utf-8",
        case_sensitive=False,
    )

    # Deployment environment
    environment: Literal["dev", "staging", "production"] = "dev"

    # Gateway identity
    gateway_id: str = "gw-dev-001"

    # Backend connection
    backend_url: str = "http://localhost:4001"
    backend_api_key: str = ""

    # Inbound API authentication (required in non-dev environments)
    api_key: str = ""

    @model_validator(mode="after")
    def _enforce_secure_backend_config(self) -> "GatewaySettings":
        """Require HTTPS and an API key for non-dev environments."""
        if self.environment != "dev":
            if not self.backend_url.startswith("https://"):
                raise ValueError(
                    f"backend_url must use HTTPS in {self.environment} "
                    f"(got {self.backend_url!r}). Set MESHTASTIC_GW_BACKEND_URL "
                    f"to an https:// URL or set MESHTASTIC_GW_ENVIRONMENT=dev "
                    f"for local development."
                )
            if not self.backend_api_key:
                raise ValueError(
                    f"backend_api_key must not be empty in {self.environment}. "
                    f"Set MESHTASTIC_GW_BACKEND_API_KEY or set "
                    f"MESHTASTIC_GW_ENVIRONMENT=dev for local development."
                )
            if not self.api_key:
                raise ValueError(
                    f"api_key must not be empty in {self.environment}. "
                    f"Set MESHTASTIC_GW_API_KEY or set "
                    f"MESHTASTIC_GW_ENVIRONMENT=dev for local development."
                )
        return self

    # Meshtastic connection mode
    meshtastic_mode: Literal["mock", "serial", "tcp"] = "mock"
    meshtastic_host: str = "localhost"
    meshtastic_port: int = 4403
    meshtastic_serial_port: str = "/dev/ttyUSB0"

    # Service settings
    poll_interval_seconds: int = 5
    observation_interval_seconds: int = 30
    command_poll_interval_seconds: int = 10

    # Logging
    log_level: str = "INFO"
    log_format: Literal["json", "console"] = "console"

    # Server settings
    host: str = "127.0.0.1"
    port: int = 8001

    # Mock mode settings (for development/testing)
    mock_node_count: int = 3
    mock_location_lat: float = 37.335
    mock_location_lon: float = -121.89


# Global settings instance
settings = GatewaySettings()
