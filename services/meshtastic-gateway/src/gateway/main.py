"""
Meshtastic Gateway Service - FastAPI Application Entry Point

Bridges LoRa mesh networks with the WRFCoin backend for weather data collection
and mesh participation rewards.
"""

import asyncio
import random
from contextlib import asynccontextmanager
from datetime import datetime
from typing import AsyncIterator

import httpx
import structlog
from fastapi import Depends, FastAPI

from .api.routes import require_api_key, router, set_meshtastic_interface
from .config import settings
from .meshtastic import MockMeshtasticInterface
from .meshtastic.interface import MeshtasticInterface
from .models.payloads import MeshContext, ObservationRequest, WrfPayload

# Configure structured logging
structlog.configure(
    processors=[
        structlog.stdlib.filter_by_level,
        structlog.stdlib.add_logger_name,
        structlog.stdlib.add_log_level,
        structlog.stdlib.PositionalArgumentsFormatter(),
        structlog.processors.TimeStamper(fmt="iso"),
        structlog.processors.StackInfoRenderer(),
        structlog.processors.format_exc_info,
        structlog.processors.UnicodeDecoder(),
        structlog.processors.JSONRenderer()
        if settings.log_format == "json"
        else structlog.dev.ConsoleRenderer(),
    ],
    wrapper_class=structlog.stdlib.BoundLogger,
    context_class=dict,
    logger_factory=structlog.stdlib.LoggerFactory(),
    cache_logger_on_first_use=True,
)

logger = structlog.get_logger()

# Global state
_meshtastic_interface: MeshtasticInterface | None = None
_http_client: httpx.AsyncClient | None = None
_observation_task: asyncio.Task | None = None
_command_poll_task: asyncio.Task | None = None


def create_meshtastic_interface() -> MeshtasticInterface:
    """Factory function to create the appropriate Meshtastic interface."""
    if settings.meshtastic_mode == "mock":
        return MockMeshtasticInterface(settings)
    # Future: Add serial and TCP implementations
    # elif settings.meshtastic_mode == "serial":
    #     return SerialMeshtasticInterface(settings)
    # elif settings.meshtastic_mode == "tcp":
    #     return TcpMeshtasticInterface(settings)
    else:
        logger.warning(
            "unknown_meshtastic_mode",
            mode=settings.meshtastic_mode,
            falling_back_to="mock",
        )
        return MockMeshtasticInterface(settings)


async def forward_observation_to_backend(
    node_id: str, payload: WrfPayload, context: MeshContext
) -> bool:
    """Forward a received observation to the WRFCoin backend."""
    try:
        request = ObservationRequest(
            gatewayId=settings.gateway_id,
            meshtasticNodeId=node_id,
            wrfPayload=payload,
            meshContext=context,
            receivedAt=datetime.utcnow(),
        )

        headers = {}
        if settings.backend_api_key:
            headers["Authorization"] = f"Bearer {settings.backend_api_key}"

        response = await _http_client.post(
            f"{settings.backend_url}/api/meshtastic/observation",
            json=request.model_dump(mode="json"),
            headers=headers,
            timeout=10.0,
        )

        if response.status_code in (200, 201):
            logger.info(
                "observation_forwarded",
                node_id=node_id,
                status_code=response.status_code,
            )
            return True
        else:
            logger.warning(
                "observation_forward_failed",
                node_id=node_id,
                status_code=response.status_code,
                response=response.text[:200],
            )
            return False

    except httpx.RequestError as e:
        logger.error(
            "observation_forward_error",
            node_id=node_id,
            error=str(e),
        )
        return False


async def observation_loop(interface: MeshtasticInterface) -> None:
    """Main loop for receiving and forwarding observations."""
    logger.info("observation_loop_started")

    try:
        async for node_id, payload, context in interface.receive_packets():
            if payload.kind == "obs":
                await forward_observation_to_backend(node_id, payload, context)
            elif payload.kind == "tx":
                # TODO: Implement transaction chunk handling
                logger.debug("tx_chunk_received", node_id=node_id)
            else:
                logger.debug("unknown_payload_kind", node_id=node_id, kind=payload.kind)

    except asyncio.CancelledError:
        logger.info("observation_loop_cancelled")
        raise
    except Exception as e:
        logger.error("observation_loop_error", error=str(e))
        raise


async def command_poll_loop() -> None:
    """Poll backend for commands to execute with jitter and exponential backoff."""
    logger.info("command_poll_loop_started")

    base_interval = settings.command_poll_interval_seconds
    max_interval = base_interval * 16  # bounded backoff ceiling
    current_interval = base_interval
    consecutive_failures = 0

    while True:
        try:
            # Add jitter: ±25% of current interval
            jitter = current_interval * 0.25 * (2 * random.random() - 1)
            await asyncio.sleep(current_interval + jitter)

            headers = {}
            if settings.backend_api_key:
                headers["Authorization"] = f"Bearer {settings.backend_api_key}"

            response = await _http_client.get(
                f"{settings.backend_url}/api/meshtastic/commands",
                params={"gatewayId": settings.gateway_id},
                headers=headers,
                timeout=10.0,
            )

            if response.status_code == 200:
                data = response.json()
                commands = data.get("commands", [])
                if commands:
                    logger.info("commands_received", count=len(commands))
                    # TODO: Execute commands via Meshtastic interface

            # On success, reset backoff
            consecutive_failures = 0
            current_interval = base_interval

        except asyncio.CancelledError:
            logger.info("command_poll_loop_cancelled")
            raise
        except (httpx.RequestError, Exception) as e:
            consecutive_failures += 1
            current_interval = min(
                base_interval * (2 ** consecutive_failures), max_interval
            )
            logger.warning(
                "command_poll_error",
                consecutive_failures=consecutive_failures,
                backoff_seconds=round(current_interval, 1),
                error=str(e),
            )


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncIterator[None]:
    """
    Application lifespan manager.

    Handles startup (Meshtastic connection, background tasks) and
    shutdown (cleanup, disconnection).
    """
    global _meshtastic_interface, _http_client, _observation_task, _command_poll_task

    logger.info(
        "gateway_starting",
        gateway_id=settings.gateway_id,
        meshtastic_mode=settings.meshtastic_mode,
        backend_url=settings.backend_url,
    )

    # Create shared HTTP client
    _http_client = httpx.AsyncClient(timeout=30.0)

    # Create and connect Meshtastic interface
    _meshtastic_interface = create_meshtastic_interface()
    set_meshtastic_interface(_meshtastic_interface)

    try:
        await _meshtastic_interface.connect()
    except Exception as e:
        logger.error("meshtastic_connection_failed", error=str(e))
        # Continue running for health checks even if connection fails

    # Start background tasks
    _observation_task = asyncio.create_task(observation_loop(_meshtastic_interface))
    _command_poll_task = asyncio.create_task(command_poll_loop())

    logger.info("gateway_started")

    yield

    # Shutdown
    logger.info("gateway_stopping")

    # Cancel background tasks
    if _observation_task:
        _observation_task.cancel()
        try:
            await _observation_task
        except asyncio.CancelledError:
            pass

    if _command_poll_task:
        _command_poll_task.cancel()
        try:
            await _command_poll_task
        except asyncio.CancelledError:
            pass

    # Close shared HTTP client
    if _http_client:
        await _http_client.aclose()

    # Disconnect Meshtastic
    if _meshtastic_interface:
        await _meshtastic_interface.disconnect()

    logger.info("gateway_stopped")


# Create FastAPI application
app = FastAPI(
    title="Meshtastic Gateway Service",
    description="Bridges LoRa mesh networks with the WRFCoin backend",
    version="0.1.0",
    lifespan=lifespan,
)

# Include API routes
app.include_router(router, prefix="", tags=["Gateway"])


@app.get("/", dependencies=[Depends(require_api_key)])
async def root() -> dict:
    """Root endpoint with service information."""
    return {
        "service": "Meshtastic Gateway",
        "version": "0.1.0",
        "gateway_id": settings.gateway_id,
        "docs": "/docs",
    }


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(
        "gateway.main:app",
        host=settings.host,
        port=settings.port,
        reload=settings.log_level == "DEBUG",
    )
