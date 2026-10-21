"""
Abstract interface for Meshtastic communication.

Defines the contract that all Meshtastic implementations must follow,
allowing for mock, serial, and TCP implementations.
"""

from abc import ABC, abstractmethod
from typing import AsyncIterator, Optional, Tuple

from ..models.payloads import WrfPayload, MeshContext


class MeshtasticInterface(ABC):
    """
    Abstract base class for Meshtastic communication.

    Implementations:
    - MockMeshtasticInterface: Development/testing without hardware
    - SerialMeshtasticInterface: USB-connected Meshtastic device
    - TcpMeshtasticInterface: Network-connected Meshtastic device
    """

    @abstractmethod
    async def connect(self) -> None:
        """
        Establish connection to the Meshtastic device.

        Raises:
            ConnectionError: If connection fails
        """
        ...

    @abstractmethod
    async def disconnect(self) -> None:
        """
        Disconnect from the Meshtastic device.
        """
        ...

    @abstractmethod
    async def is_connected(self) -> bool:
        """
        Check if currently connected to the device.

        Returns:
            True if connected, False otherwise
        """
        ...

    @abstractmethod
    async def receive_packets(
        self,
    ) -> AsyncIterator[Tuple[str, WrfPayload, MeshContext]]:
        """
        Async generator that yields received WRF packets.

        Yields:
            Tuple of (node_id, payload, mesh_context)
        """
        ...

    @abstractmethod
    async def send_packet(
        self, node_id: str, payload: WrfPayload, want_ack: bool = False
    ) -> bool:
        """
        Send a WRF packet to a specific node.

        Args:
            node_id: Target Meshtastic node ID
            payload: WRF payload to send
            want_ack: Whether to request acknowledgment

        Returns:
            True if sent successfully
        """
        ...

    @abstractmethod
    async def broadcast_packet(
        self, payload: WrfPayload, want_ack: bool = False
    ) -> bool:
        """
        Broadcast a WRF packet to all nodes.

        Args:
            payload: WRF payload to broadcast
            want_ack: Whether to request acknowledgment

        Returns:
            True if sent successfully
        """
        ...

    @abstractmethod
    async def get_node_info(self, node_id: str) -> Optional[dict]:
        """
        Get information about a specific node.

        Args:
            node_id: Meshtastic node ID

        Returns:
            Node information dict or None if not found
        """
        ...

    @abstractmethod
    async def get_connected_nodes(self) -> list[dict]:
        """
        Get list of all known nodes in the mesh.

        Returns:
            List of node information dicts
        """
        ...

    @abstractmethod
    async def request_device_metrics(self, node_id: str) -> Optional[dict]:
        """
        Request device metrics from a node.

        Args:
            node_id: Meshtastic node ID

        Returns:
            Device metrics dict or None
        """
        ...

    @abstractmethod
    async def request_environment_metrics(self, node_id: str) -> Optional[dict]:
        """
        Request environmental metrics from a node.

        Args:
            node_id: Meshtastic node ID

        Returns:
            Environmental metrics dict or None
        """
        ...
