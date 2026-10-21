"""
Tests for Meshtastic Gateway Service.
"""

import pytest
from fastapi.testclient import TestClient

from src.gateway.config import GatewaySettings, settings
from src.gateway.main import app
from src.gateway.meshtastic.mock import MockMeshtasticInterface
from src.gateway.models.payloads import (
    MeshContext,
    ObservationRequest,
    WrfObservationBody,
    WrfPayload,
)


class TestConfiguration:
    """Tests for configuration loading."""

    def test_default_settings(self):
        """Test that default settings are loaded correctly."""
        settings = GatewaySettings()
        assert settings.gateway_id == "gw-dev-001"
        assert settings.meshtastic_mode == "mock"
        assert settings.port == 8001

    def test_settings_with_env_prefix(self, monkeypatch):
        """Test that environment variables with prefix are loaded."""
        monkeypatch.setenv("MESHTASTIC_GW_GATEWAY_ID", "gw-test-123")
        monkeypatch.setenv("MESHTASTIC_GW_BACKEND_URL", "http://test:9000")

        settings = GatewaySettings()
        assert settings.gateway_id == "gw-test-123"
        assert settings.backend_url == "http://test:9000"


class TestMockMeshtasticInterface:
    """Tests for the mock Meshtastic interface."""

    @pytest.fixture
    def mock_interface(self):
        """Create a mock interface for testing."""
        settings = GatewaySettings()
        return MockMeshtasticInterface(settings)

    @pytest.mark.asyncio
    async def test_connect(self, mock_interface):
        """Test mock connection."""
        await mock_interface.connect()
        assert await mock_interface.is_connected() is True

    @pytest.mark.asyncio
    async def test_disconnect(self, mock_interface):
        """Test mock disconnection."""
        await mock_interface.connect()
        await mock_interface.disconnect()
        assert await mock_interface.is_connected() is False

    @pytest.mark.asyncio
    async def test_get_connected_nodes(self, mock_interface):
        """Test getting list of mock nodes."""
        await mock_interface.connect()
        nodes = await mock_interface.get_connected_nodes()
        assert len(nodes) > 0
        assert all("node_id" in node for node in nodes)

    @pytest.mark.asyncio
    async def test_get_node_info(self, mock_interface):
        """Test getting info for a specific node."""
        await mock_interface.connect()
        nodes = await mock_interface.get_connected_nodes()
        node_id = nodes[0]["node_id"]

        node_info = await mock_interface.get_node_info(node_id)
        assert node_info is not None
        assert node_info["node_id"] == node_id

    @pytest.mark.asyncio
    async def test_get_node_info_not_found(self, mock_interface):
        """Test getting info for non-existent node."""
        await mock_interface.connect()
        node_info = await mock_interface.get_node_info("!nonexistent")
        assert node_info is None

    @pytest.mark.asyncio
    async def test_send_packet(self, mock_interface):
        """Test sending a packet to a node."""
        await mock_interface.connect()
        nodes = await mock_interface.get_connected_nodes()
        node_id = nodes[0]["node_id"]

        payload = WrfPayload(
            t="wrf",
            v=1,
            kind="ping",
            body={},
        )
        result = await mock_interface.send_packet(node_id, payload)
        assert result is True

    @pytest.mark.asyncio
    async def test_send_packet_unknown_node(self, mock_interface):
        """Test sending to unknown node fails."""
        await mock_interface.connect()

        payload = WrfPayload(
            t="wrf",
            v=1,
            kind="ping",
            body={},
        )
        result = await mock_interface.send_packet("!unknown", payload)
        assert result is False


class TestPayloadModels:
    """Tests for Pydantic payload models."""

    def test_wrf_observation_body(self):
        """Test WrfObservationBody validation."""
        body = WrfObservationBody(
            ts=1704067200,
            lat=37.335,
            lon=-121.89,
            tmp=15.5,
            hum=65.0,
            prs=1013.25,
            bat=80,
        )
        assert body.ts == 1704067200
        assert body.tmp == 15.5

    def test_wrf_observation_body_bounds(self):
        """Test WrfObservationBody field bounds."""
        with pytest.raises(ValueError):
            WrfObservationBody(
                ts=1704067200,
                lat=91.0,  # Invalid: > 90
                lon=-121.89,
            )

    def test_wrf_payload(self):
        """Test WrfPayload envelope."""
        body = WrfObservationBody(
            ts=1704067200,
            lat=37.335,
            lon=-121.89,
        )
        payload = WrfPayload(t="wrf", v=1, kind="obs", body=body)
        assert payload.kind == "obs"
        assert payload.v == 1

    def test_mesh_context(self):
        """Test MeshContext model."""
        context = MeshContext(
            rssi=-85,
            snr=7.5,
            hopCount=2,
            neighborCount=5,
        )
        assert context.rssi == -85
        assert context.snr == 7.5

    def test_observation_request(self):
        """Test ObservationRequest model."""
        from datetime import datetime

        body = WrfObservationBody(ts=1704067200, lat=37.335, lon=-121.89)
        payload = WrfPayload(t="wrf", v=1, kind="obs", body=body)
        context = MeshContext(rssi=-85, snr=7.5)

        request = ObservationRequest(
            gatewayId="gw-test-001",
            meshtasticNodeId="!12345678",
            wrfPayload=payload,
            meshContext=context,
            receivedAt=datetime.utcnow(),
        )
        assert request.gatewayId == "gw-test-001"


TEST_API_KEY = "test-gateway-key-869"


class TestAPIEndpoints:
    """Tests for FastAPI endpoints."""

    @pytest.fixture
    def client(self):
        """Create test client (no api_key configured — dev mode)."""
        return TestClient(app)

    def test_root_endpoint(self, client):
        """Test root endpoint returns service info (dev mode, no auth)."""
        response = client.get("/")
        assert response.status_code == 200
        data = response.json()
        assert data["service"] == "Meshtastic Gateway"
        assert "version" in data

    def test_health_endpoint(self, client):
        """Test health endpoint."""
        response = client.get("/health")
        assert response.status_code == 200
        data = response.json()
        assert "status" in data
        assert "gateway_id" in data
        assert "timestamp" in data

    def test_status_endpoint(self, client):
        """Test status endpoint is reachable without auth in dev mode."""
        response = client.get("/status")
        # 200 if interface is up, 503 if not — but never 401/403
        assert response.status_code in (200, 503)

    def test_nodes_endpoint(self, client):
        """Test nodes endpoint is reachable without auth in dev mode."""
        response = client.get("/nodes")
        # 200 if interface is up, 503 if not — but never 401/403
        assert response.status_code in (200, 503)


class TestAPIAuthentication:
    """Tests for API key authentication on gateway endpoints."""

    @pytest.fixture(autouse=True)
    def _set_api_key(self, monkeypatch):
        """Enable API key enforcement for all tests in this class."""
        monkeypatch.setattr(settings, "api_key", TEST_API_KEY)

    @pytest.fixture
    def client(self):
        return TestClient(app)

    @pytest.fixture
    def auth_headers(self):
        return {"Authorization": f"Bearer {TEST_API_KEY}"}

    # -- /health stays open --------------------------------------------------

    def test_health_no_auth_still_works(self, client):
        """Health endpoint must remain unauthenticated for container healthchecks."""
        response = client.get("/health")
        assert response.status_code == 200

    # -- root / --------------------------------------------------------------

    def test_root_unauthenticated_returns_401(self, client):
        response = client.get("/")
        assert response.status_code == 401

    def test_root_wrong_key_returns_403(self, client):
        response = client.get("/", headers={"Authorization": "Bearer wrong-key"})
        assert response.status_code == 403

    def test_root_authenticated_returns_200(self, client, auth_headers):
        response = client.get("/", headers=auth_headers)
        assert response.status_code == 200

    # -- /status -------------------------------------------------------------

    def test_status_unauthenticated_returns_401(self, client):
        response = client.get("/status")
        assert response.status_code == 401

    def test_status_wrong_key_returns_403(self, client):
        response = client.get("/status", headers={"Authorization": "Bearer wrong-key"})
        assert response.status_code == 403

    def test_status_authenticated_passes_auth(self, client, auth_headers):
        response = client.get("/status", headers=auth_headers)
        # Auth passes — may get 503 if interface not initialized, but not 401/403
        assert response.status_code not in (401, 403)

    # -- /nodes --------------------------------------------------------------

    def test_nodes_unauthenticated_returns_401(self, client):
        response = client.get("/nodes")
        assert response.status_code == 401

    def test_nodes_authenticated_passes_auth(self, client, auth_headers):
        response = client.get("/nodes", headers=auth_headers)
        # Auth passes — may get 503 if interface not initialized, but not 401/403
        assert response.status_code not in (401, 403)

    # -- /nodes/{node_id} ----------------------------------------------------

    def test_node_detail_unauthenticated_returns_401(self, client):
        response = client.get("/nodes/some-node")
        assert response.status_code == 401

    def test_node_detail_authenticated(self, client, auth_headers):
        # Node won't exist, but auth should pass (404 not 401)
        response = client.get("/nodes/nonexistent", headers=auth_headers)
        assert response.status_code != 401
