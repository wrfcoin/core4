"""
Mock Meshtastic interface for development without real hardware.

Generates simulated weather observations and mesh network data
for testing the gateway service.
"""

import asyncio
import random
import time
from typing import AsyncIterator, Optional, Tuple

import structlog

from ..config import GatewaySettings
from ..models.payloads import (
    MeshContext,
    WrfObservationBody,
    WrfPayload,
)
from .interface import MeshtasticInterface

logger = structlog.get_logger()


class MockMeshtasticInterface(MeshtasticInterface):
    """
    Mock implementation for development without real LoRa hardware.

    Generates realistic weather observations and simulates mesh network behavior.
    """

    def __init__(self, settings: GatewaySettings):
        self.settings = settings
        self._connected = False
        self._nodes: dict[str, dict] = {}
        self._initialize_mock_nodes()

    def _initialize_mock_nodes(self) -> None:
        """Initialize mock mesh nodes with varied characteristics."""
        base_lat = self.settings.mock_location_lat
        base_lon = self.settings.mock_location_lon

        for i in range(self.settings.mock_node_count):
            node_id = f"!{random.randint(10000000, 99999999):08x}"
            self._nodes[node_id] = {
                "node_id": node_id,
                "user": {
                    "short_name": f"WRF{i+1}",
                    "long_name": f"WRFCoin Weather Node {i+1}",
                    "hw_model": "MUZI_R1_NEO",
                },
                "position": {
                    "latitude": base_lat + random.uniform(-0.01, 0.01),
                    "longitude": base_lon + random.uniform(-0.01, 0.01),
                    "altitude": random.randint(10, 100),
                },
                "device_metrics": {
                    "battery_level": random.randint(60, 100),
                    "voltage": round(random.uniform(3.5, 4.2), 2),
                    "uptime_seconds": random.randint(3600, 86400),
                },
                "last_heard": int(time.time()),
            }

        logger.info(
            "mock_nodes_initialized",
            node_count=len(self._nodes),
            node_ids=list(self._nodes.keys()),
        )

    async def connect(self) -> None:
        """Simulate connection to Meshtastic device."""
        await asyncio.sleep(0.1)  # Simulate connection delay
        self._connected = True
        logger.info("mock_meshtastic_connected", gateway_id=self.settings.gateway_id)

    async def disconnect(self) -> None:
        """Simulate disconnection from Meshtastic device."""
        self._connected = False
        logger.info("mock_meshtastic_disconnected")

    async def is_connected(self) -> bool:
        """Check if mock interface is connected."""
        return self._connected

    def _generate_mock_observation(self, node_id: str) -> WrfPayload:
        """Generate a realistic mock weather observation."""
        node = self._nodes.get(node_id, {})
        position = node.get("position", {})

        # Generate realistic weather data with some variation
        base_temp = 15 + random.gauss(0, 5)  # Base temp around 15C
        humidity = 50 + random.gauss(0, 20)  # Base humidity around 50%
        humidity = max(0, min(100, humidity))  # Clamp to valid range

        body = WrfObservationBody(
            ts=int(time.time()),
            lat=position.get("latitude", self.settings.mock_location_lat),
            lon=position.get("longitude", self.settings.mock_location_lon),
            tmp=round(base_temp, 1),
            hum=round(humidity, 1),
            prs=round(1013.25 + random.gauss(0, 5), 1),
            bat=node.get("device_metrics", {}).get("battery_level", 80),
            alt=position.get("altitude"),
        )

        return WrfPayload(t="wrf", v=1, kind="obs", body=body)

    def _generate_mesh_context(self) -> MeshContext:
        """Generate realistic mesh network context."""
        return MeshContext(
            rssi=random.randint(-120, -60),
            snr=round(random.uniform(-5, 15), 1),
            hopCount=random.randint(0, 3),
            neighborCount=random.randint(1, 10),
        )

    async def receive_packets(
        self,
    ) -> AsyncIterator[Tuple[str, WrfPayload, MeshContext]]:
        """
        Generate mock weather observations at configured intervals.

        Yields observations from random nodes in the mock network.
        """
        while self._connected:
            await asyncio.sleep(self.settings.observation_interval_seconds)

            if not self._connected:
                break

            # Pick a random node to generate observation from
            node_id = random.choice(list(self._nodes.keys()))
            payload = self._generate_mock_observation(node_id)
            context = self._generate_mesh_context()

            logger.debug(
                "mock_observation_generated",
                node_id=node_id,
                temperature=payload.body.tmp if isinstance(payload.body, WrfObservationBody) else None,
            )

            yield (node_id, payload, context)

    async def send_packet(
        self, node_id: str, payload: WrfPayload, want_ack: bool = False
    ) -> bool:
        """Simulate sending a packet to a node."""
        if node_id not in self._nodes:
            logger.warning("mock_send_unknown_node", node_id=node_id)
            return False

        await asyncio.sleep(0.1)  # Simulate transmission delay
        logger.debug("mock_packet_sent", node_id=node_id, kind=payload.kind)
        return True

    async def broadcast_packet(
        self, payload: WrfPayload, want_ack: bool = False
    ) -> bool:
        """Simulate broadcasting a packet to all nodes."""
        await asyncio.sleep(0.1)  # Simulate transmission delay
        logger.debug("mock_packet_broadcast", kind=payload.kind)
        return True

    async def get_node_info(self, node_id: str) -> Optional[dict]:
        """Get information about a mock node."""
        return self._nodes.get(node_id)

    async def get_connected_nodes(self) -> list[dict]:
        """Get list of all mock nodes."""
        return list(self._nodes.values())

    async def request_device_metrics(self, node_id: str) -> Optional[dict]:
        """Get mock device metrics for a node."""
        node = self._nodes.get(node_id)
        if not node:
            return None

        # Update battery level (simulate discharge)
        metrics = node.get("device_metrics", {})
        metrics["battery_level"] = max(0, metrics.get("battery_level", 80) - random.randint(0, 1))
        metrics["uptime_seconds"] = metrics.get("uptime_seconds", 0) + 60

        return metrics

    async def request_environment_metrics(self, node_id: str) -> Optional[dict]:
        """Get mock environmental metrics for a node."""
        node = self._nodes.get(node_id)
        if not node:
            return None

        return {
            "temperature": round(15 + random.gauss(0, 5), 1),
            "humidity": round(50 + random.gauss(0, 20), 1),
            "pressure": round(1013.25 + random.gauss(0, 5), 1),
            "air_quality": random.randint(0, 100),
        }
