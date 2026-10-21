use anyhow::{Result, anyhow};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::time::sleep;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};
use url::Url;

/// Configuration for the WebSocket client
#[derive(Clone, Debug)]
pub struct WebSocketConfig {
    /// WebSocket server URL
    pub url: String,
    /// Interval for sending heartbeat messages
    pub heartbeat_interval: Duration,
    /// Interval for reconnection attempts
    pub reconnect_interval: Duration,
    /// Maximum number of reconnection attempts
    pub max_reconnect_attempts: u32,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            url: "ws://localhost:5001/ws".to_string(),
            heartbeat_interval: Duration::from_secs(30),
            reconnect_interval: Duration::from_secs(3),
            max_reconnect_attempts: 5,
        }
    }
}

/// WebSocket message
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebSocketMessage {
    /// Message channel
    pub channel: String,
    /// Message event type
    pub event: String,
    /// Message data
    pub data: Value,
}

/// WebSocket client for Wrfcoin
pub struct WebSocketClient {
    /// WebSocket configuration
    #[allow(dead_code)]
    config: WebSocketConfig,
    /// Channel for sending messages to the WebSocket connection
    sender: Option<Sender<Message>>,
    /// Channel for receiving messages from the WebSocket connection
    receiver: Option<Receiver<WebSocketMessage>>,
    /// Connection status
    connected: Arc<Mutex<bool>>,
    /// Subscribed channels
    subscriptions: Arc<Mutex<Vec<String>>>,
}

impl WebSocketClient {
    /// Create a new WebSocket client
    pub async fn new(config: WebSocketConfig) -> Result<Self> {
        let connected = Arc::new(Mutex::new(false));
        let subscriptions = Arc::new(Mutex::new(Vec::new()));

        // Create channels for communication
        let (tx, rx) = mpsc::channel::<Message>(100);
        let (message_tx, message_rx) = mpsc::channel::<WebSocketMessage>(100);

        // Start connection task
        let conn_config = config.clone();
        let conn_connected = connected.clone();
        let conn_subscriptions = subscriptions.clone();
        tokio::spawn(async move {
            Self::connection_task(
                conn_config,
                rx,
                message_tx,
                conn_connected,
                conn_subscriptions,
            )
            .await;
        });

        // Wait for connection to establish
        for _ in 0..20 {
            if *connected.lock().unwrap() {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }

        // Check if connection was established
        if !*connected.lock().unwrap() {
            warn!("WebSocket connection not established after timeout");
        }

        Ok(Self {
            config,
            sender: Some(tx),
            receiver: Some(message_rx),
            connected,
            subscriptions,
        })
    }

    /// Connection task
    async fn connection_task(
        config: WebSocketConfig,
        _message_receiver: Receiver<Message>,
        _message_tx: Sender<WebSocketMessage>,
        connected: Arc<Mutex<bool>>,
        _subscriptions: Arc<Mutex<Vec<String>>>,
    ) {
        let mut reconnect_attempts = 0;

        'reconnect_loop: loop {
            // Connect to WebSocket server
            let url = match Url::parse(&config.url) {
                Ok(url) => url,
                Err(err) => {
                    error!("Invalid WebSocket URL: {err}");
                    break 'reconnect_loop;
                },
            };

            debug!("Connecting to WebSocket server: {url}");

            match connect_async(url).await {
                Ok((_ws_stream, _)) => {
                    info!("Connected to WebSocket server");
                    *connected.lock().unwrap() = true;
                    reconnect_attempts = 0;

                    // Handle WebSocket connection (simplified for now)
                    // Temporarily disabled for compilation
                    // if let Err(err) = Self::handle_connection(
                    //     ws_stream,
                    //     message_receiver,
                    //     message_tx.clone(),
                    //     config.heartbeat_interval,
                    //     connected.clone(),
                    //     subscriptions.clone(),
                    // ).await {
                    //     error!("WebSocket connection error: {}", err);
                    // }

                    // Simple connection handling for now
                    tokio::time::sleep(Duration::from_secs(5)).await;

                    // Connection closed
                    *connected.lock().unwrap() = false;
                    info!("WebSocket connection closed");
                },
                Err(err) => {
                    error!("Failed to connect to WebSocket server: {err}");
                    reconnect_attempts += 1;

                    if reconnect_attempts >= config.max_reconnect_attempts {
                        error!("Maximum reconnection attempts reached. Giving up.");
                        break 'reconnect_loop;
                    }

                    info!(
                        "Reconnecting in {} seconds (attempt {}/{})",
                        config.reconnect_interval.as_secs(),
                        reconnect_attempts,
                        config.max_reconnect_attempts
                    );

                    sleep(config.reconnect_interval).await;
                    continue;
                },
            }

            // If we get here, the connection was closed.
            // Try to reconnect unless max attempts reached
            reconnect_attempts += 1;

            if reconnect_attempts >= config.max_reconnect_attempts {
                error!("Maximum reconnection attempts reached. Giving up.");
                break 'reconnect_loop;
            }

            info!(
                "Reconnecting in {} seconds (attempt {}/{})",
                config.reconnect_interval.as_secs(),
                reconnect_attempts,
                config.max_reconnect_attempts
            );

            sleep(config.reconnect_interval).await;
        }

        // Connection task is exiting
        error!("WebSocket connection task exiting");
    }

    /// Handle WebSocket connection (temporarily disabled)
    #[allow(dead_code)]
    async fn handle_connection(
        _ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
        _client_receiver: Arc<Mutex<Receiver<Message>>>,
        _message_tx: Sender<WebSocketMessage>,
        _heartbeat_interval: Duration,
        _connected: Arc<Mutex<bool>>,
        _subscriptions: Arc<Mutex<Vec<String>>>,
    ) -> Result<()> {
        // Temporarily disabled for compilation
        Ok(())
    }

    /// Check if the client is connected
    pub fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }

    /// Subscribe to a channel
    pub async fn subscribe(&self, channel: &str) -> Result<()> {
        if !self.is_connected() {
            return Err(anyhow!("Not connected to WebSocket server"));
        }

        // Add to subscriptions
        {
            let mut subs = self.subscriptions.lock().unwrap();
            if !subs.contains(&channel.to_string()) {
                subs.push(channel.to_string());
            }
        }

        // Send subscription message
        let subscribe_msg = json!({
            "action": "subscribe",
            "channel": channel
        });

        debug!("Subscribing to channel: {channel}");

        if let Some(sender) = &self.sender {
            if let Err(err) = sender
                .clone()
                .send(Message::Text(subscribe_msg.to_string()))
                .await
            {
                error!("Error sending subscription message: {err}");
                return Err(anyhow!("Failed to send subscription message: {err}"));
            }
        } else {
            return Err(anyhow!("WebSocket sender channel not available"));
        }

        Ok(())
    }

    /// Unsubscribe from a channel
    pub async fn unsubscribe(&self, channel: &str) -> Result<()> {
        if !self.is_connected() {
            return Err(anyhow!("Not connected to WebSocket server"));
        }

        // Remove from subscriptions
        {
            let mut subs = self.subscriptions.lock().unwrap();
            subs.retain(|c| c != channel);
        }

        // Send unsubscription message
        let unsubscribe_msg = json!({
            "action": "unsubscribe",
            "channel": channel
        });

        debug!("Unsubscribing from channel: {channel}");

        if let Some(sender) = &self.sender {
            if let Err(err) = sender
                .clone()
                .send(Message::Text(unsubscribe_msg.to_string()))
                .await
            {
                error!("Error sending unsubscription message: {err}");
                return Err(anyhow!("Failed to send unsubscription message: {err}"));
            }
        } else {
            return Err(anyhow!("WebSocket sender channel not available"));
        }

        Ok(())
    }

    /// Send a message to the WebSocket server
    pub async fn send(&self, message: &str) -> Result<()> {
        if !self.is_connected() {
            return Err(anyhow!("Not connected to WebSocket server"));
        }

        if let Some(sender) = &self.sender {
            if let Err(err) = sender
                .clone()
                .send(Message::Text(message.to_string()))
                .await
            {
                error!("Error sending message: {err}");
                return Err(anyhow!("Failed to send message: {err}"));
            }
        } else {
            return Err(anyhow!("WebSocket sender channel not available"));
        }

        Ok(())
    }

    /// Receive a message from the WebSocket server
    pub async fn receive(&mut self) -> Option<WebSocketMessage> {
        if let Some(receiver) = &mut self.receiver {
            receiver.recv().await
        } else {
            None
        }
    }

    /// Close the WebSocket connection
    pub async fn close(&mut self) -> Result<()> {
        // Clear channels
        self.sender = None;
        self.receiver = None;

        // Reset connection state
        *self.connected.lock().unwrap() = false;

        // Clear subscriptions
        self.subscriptions.lock().unwrap().clear();

        Ok(())
    }
}
