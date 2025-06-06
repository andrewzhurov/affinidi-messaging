use crate::{config::Config, errors::ATMError, ATM};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};
use ws_handler::WSCommand;

pub mod sending;
pub mod ws_handler;

impl ATM {
    /// Starts websocket connection to the ATM API
    /// Example:
    /// ```ignore
    /// use affinidi_messaging_sdk::ATM;
    /// use affinidi_messaging_sdk::config::Config;
    ///
    /// // Configure and create ATM instance
    /// let config = Config::builder().build();
    /// let atm = ATM::new(config).await?;
    ///
    /// // Get a websocket connection (should be mutable as it will be used to send messages)
    /// let mut ws = atm.get_websocket().await?;
    /// ```
    pub async fn start_websocket_task(&mut self) -> Result<(), ATMError> {
        // Some hackery to get around the Rust lifetimes by various items in the SDK
        // Create a copy of ATM with owned values
        let mut config = Config {
            ssl_certificates: Vec::new(),
            ..self.config.clone()
        };

        for cert in &self.config.ssl_certificates {
            config.ssl_certificates.push(cert.clone().into_owned())
        }

        let mut atm = ATM {
            config,
            did_resolver: self.did_resolver.clone(),
            secrets_resolver: self.secrets_resolver.clone(),
            client: self.client.clone(),
            authenticated: self.authenticated,
            jwt_tokens: self.jwt_tokens.clone(),
            ws_connector: self.ws_connector.clone(),
            ws_enabled: self.ws_enabled,
            ws_handler: None,
            ws_send_stream: None,
            ws_recv_stream: None,
        };

        debug!("secrets: {}", atm.secrets_resolver.len());

        // Create a new channel with a capacity of at most 32. This communicates from SDK to the websocket handler
        let (tx, mut rx) = mpsc::channel::<WSCommand>(32);
        self.ws_send_stream = Some(tx);

        // Create a new channel with a capacity of at most 32. This communicates from websocket handler to SDK
        let (tx2, rx2) = mpsc::channel::<WSCommand>(32);
        self.ws_recv_stream = Some(rx2);

        // Start the websocket connection
        //let mut web_socket = self._create_socket(&mut atm).await?;
        //self.ws_websocket = Some(self._create_socket(&mut atm).await?);
        // self.ws_websocket = Some(web_socket);

        self.ws_handler = Some(tokio::spawn(async move {
            let _ = ATM::ws_handler(&mut atm, &mut rx, &tx2).await;
        }));

        if let Some(ws_recv) = self.ws_recv_stream.as_mut() {
            // Wait for Started message
            if let Some(msg) = ws_recv.recv().await {
                match msg {
                    WSCommand::Started => {
                        debug!("Websocket connection started");
                    }
                    _ => {
                        warn!("Unknown message from ws_handler: {:?}", msg);
                    }
                }
            }
        }

        debug!("Websocket connection and handler started");

        Ok(())
    }

    /// Close the WebSocket task gracefully
    pub async fn abort_websocket_task(&mut self) -> Result<(), ATMError> {
        if let Some(channel) = self.ws_send_stream.as_mut() {
            let _ = channel.send(WSCommand::Exit).await;
        }

        Ok(())
    }
}
