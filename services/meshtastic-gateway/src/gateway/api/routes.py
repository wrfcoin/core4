"""
FastAPI routes for gateway service health and status endpoints.
"""

from datetime import datetime
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request

from ..config import settings
from ..meshtastic import MeshtasticInterface

router = APIRouter()


async def require_api_key(request: Request) -> None:
    """Validate Bearer token against the configured API key.

    Skipped when no api_key is configured (dev mode).
    """
    if not settings.api_key:
        return
    auth = request.headers.get("Authorization", "")
    if not auth.startswith("Bearer "):
        raise HTTPException(status_code=401, detail="Missing or malformed Authorization header")
    token = auth[len("Bearer "):]
    if token != settings.api_key:
        raise HTTPException(status_code=403, detail="Invalid API key")


# Global reference to the Meshtastic interface (set by main.py)
_meshtastic_interface: MeshtasticInterface | None = None


def set_meshtastic_interface(interface: MeshtasticInterface) -> None:
    """Set the global Meshtastic interface reference."""
    global _meshtastic_interface
    _meshtastic_interface = interface


@router.get("/health")
async def health_check() -> dict[str, Any]:
    """
    Health check endpoint for container orchestration.

    Returns basic health status of the gateway service.
    """
    is_connected = False
    if _meshtastic_interface:
        is_connected = await _meshtastic_interface.is_connected()

    return {
        "status": "healthy" if is_connected else "degraded",
        "timestamp": datetime.utcnow().isoformat() + "Z",
        "gateway_id": settings.gateway_id,
        "meshtastic_mode": settings.meshtastic_mode,
        "meshtastic_connected": is_connected,
        "version": "0.1.0",
    }


@router.get("/status", dependencies=[Depends(require_api_key)])
async def get_status() -> dict[str, Any]:
    """
    Detailed status endpoint with mesh network information.
    """
    if not _meshtastic_interface:
        raise HTTPException(status_code=503, detail="Meshtastic interface not initialized")

    is_connected = await _meshtastic_interface.is_connected()
    nodes = await _meshtastic_interface.get_connected_nodes() if is_connected else []

    return {
        "gateway_id": settings.gateway_id,
        "timestamp": datetime.utcnow().isoformat() + "Z",
        "meshtastic": {
            "mode": settings.meshtastic_mode,
            "connected": is_connected,
            "node_count": len(nodes),
        },
        "backend": {
            "url": settings.backend_url,
            "poll_interval_seconds": settings.poll_interval_seconds,
        },
        "config": {
            "observation_interval_seconds": settings.observation_interval_seconds,
            "command_poll_interval_seconds": settings.command_poll_interval_seconds,
            "log_level": settings.log_level,
        },
    }


@router.get("/nodes", dependencies=[Depends(require_api_key)])
async def get_nodes() -> dict[str, Any]:
    """
    List all known nodes in the mesh network.
    """
    if not _meshtastic_interface:
        raise HTTPException(status_code=503, detail="Meshtastic interface not initialized")

    is_connected = await _meshtastic_interface.is_connected()
    if not is_connected:
        raise HTTPException(status_code=503, detail="Meshtastic not connected")

    nodes = await _meshtastic_interface.get_connected_nodes()

    return {
        "gateway_id": settings.gateway_id,
        "timestamp": datetime.utcnow().isoformat() + "Z",
        "node_count": len(nodes),
        "nodes": nodes,
    }


@router.get("/nodes/{node_id}", dependencies=[Depends(require_api_key)])
async def get_node(node_id: str) -> dict[str, Any]:
    """
    Get detailed information about a specific node.
    """
    if not _meshtastic_interface:
        raise HTTPException(status_code=503, detail="Meshtastic interface not initialized")

    node_info = await _meshtastic_interface.get_node_info(node_id)
    if not node_info:
        raise HTTPException(status_code=404, detail=f"Node {node_id} not found")

    device_metrics = await _meshtastic_interface.request_device_metrics(node_id)
    env_metrics = await _meshtastic_interface.request_environment_metrics(node_id)

    return {
        "node_id": node_id,
        "timestamp": datetime.utcnow().isoformat() + "Z",
        "info": node_info,
        "device_metrics": device_metrics,
        "environment_metrics": env_metrics,
    }
