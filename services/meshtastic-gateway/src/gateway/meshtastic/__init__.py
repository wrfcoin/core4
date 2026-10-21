"""
Meshtastic interface module.

Provides abstract interface and implementations for communicating with
Meshtastic mesh networks.
"""

from .interface import MeshtasticInterface
from .mock import MockMeshtasticInterface

__all__ = ["MeshtasticInterface", "MockMeshtasticInterface"]
