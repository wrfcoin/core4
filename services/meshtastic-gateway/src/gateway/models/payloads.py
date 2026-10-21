"""
Pydantic models for WRFCoin payloads transmitted over LoRa mesh.

These models define the WRF payload envelope format and specific payload types
for weather observations and transaction relay.
"""

from datetime import datetime
from typing import Literal, Optional, Union
from pydantic import BaseModel, Field


class WrfObservationBody(BaseModel):
    """Weather observation body payload."""

    ts: int = Field(..., description="Unix timestamp")
    lat: float = Field(..., ge=-90, le=90, description="Latitude")
    lon: float = Field(..., ge=-180, le=180, description="Longitude")
    tmp: Optional[float] = Field(None, ge=-100, le=100, description="Temperature (C)")
    hum: Optional[float] = Field(None, ge=0, le=100, description="Humidity (%)")
    prs: Optional[float] = Field(None, ge=800, le=1200, description="Pressure (hPa)")
    bat: Optional[int] = Field(None, ge=0, le=100, description="Battery (%)")
    alt: Optional[float] = Field(None, description="Altitude (m)")
    lux: Optional[float] = Field(None, ge=0, description="Light (lux)")
    pm25: Optional[float] = Field(None, ge=0, description="PM2.5 (ug/m3)")
    pm10: Optional[float] = Field(None, ge=0, description="PM10 (ug/m3)")
    voc: Optional[int] = Field(None, ge=0, description="VOC index")
    co2: Optional[int] = Field(None, ge=0, description="CO2 equivalent (ppm)")


class TxChunk(BaseModel):
    """Transaction chunk for multi-packet transactions."""

    index: int = Field(..., ge=0, description="Chunk index")
    total: int = Field(..., ge=1, description="Total chunks")
    data: str = Field(..., description="Base64-encoded chunk data")


class WrfTransactionBody(BaseModel):
    """Transaction relay body payload."""

    txId: str = Field(..., description="Transaction ID")
    sender: str = Field(..., alias="from", description="Sender address")
    to: str = Field(..., description="Recipient address")
    amount: str = Field(..., description="Amount as string")
    nonce: int = Field(..., ge=0, description="Transaction nonce")
    signature: str = Field(..., description="Transaction signature")
    chunk: TxChunk = Field(..., description="Chunk information")


class WrfPayload(BaseModel):
    """
    WRFCoin payload envelope for LoRa transmission.

    Format:
    {
        "t": "wrf",          # type marker
        "v": 1,              # schema version
        "kind": "obs",       # "obs" | "tx" | "reg" | "ping"
        "body": { ... }      # type-specific body
    }
    """

    t: Literal["wrf"] = Field("wrf", description="Type marker")
    v: int = Field(1, ge=1, description="Schema version")
    kind: Literal["obs", "tx", "reg", "ping"] = Field(..., description="Payload kind")
    body: Union[WrfObservationBody, WrfTransactionBody, dict] = Field(
        ..., description="Payload body"
    )


class MeshContext(BaseModel):
    """Mesh network context for received packets."""

    rssi: Optional[int] = Field(None, description="Received signal strength (dBm)")
    snr: Optional[float] = Field(None, description="Signal-to-noise ratio")
    hopCount: Optional[int] = Field(None, ge=0, description="Number of hops")
    neighborCount: Optional[int] = Field(None, ge=0, description="Neighbor node count")


class ObservationRequest(BaseModel):
    """Request payload for submitting observations to backend."""

    gatewayId: str = Field(..., description="Gateway identifier")
    meshtasticNodeId: str = Field(..., description="Meshtastic node ID")
    wrfPayload: WrfPayload = Field(..., description="WRF payload envelope")
    meshContext: Optional[MeshContext] = Field(None, description="Mesh network context")
    receivedAt: datetime = Field(..., description="Timestamp when received")


class TxChunkRequest(BaseModel):
    """Request payload for submitting transaction chunks to backend."""

    gatewayId: str = Field(..., description="Gateway identifier")
    meshtasticNodeId: str = Field(..., description="Meshtastic node ID")
    wrfPayload: WrfPayload = Field(..., description="WRF payload envelope")
    receivedAt: datetime = Field(..., description="Timestamp when received")


class CommandResponse(BaseModel):
    """Command from backend for gateway to execute."""

    id: str = Field(..., description="Command ID")
    kind: str = Field(..., description="Command type")
    targetNodeId: Optional[str] = Field(None, description="Target node ID")
    wrfPayload: Optional[dict] = Field(None, description="Payload to send")
    expiresAt: datetime = Field(..., description="Command expiration time")


class CommandsResponse(BaseModel):
    """Response containing pending commands for gateway."""

    commands: list[CommandResponse] = Field(default_factory=list)
