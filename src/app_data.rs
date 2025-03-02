// PortRedirect
//
// License: GPL-3.0-only

use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use secrecy::SecretString;

// Storage for application data for handler functions.
#[derive(Clone, Debug)]
pub struct ServerAppData {
    // PSK for authenticating client connections.
    pub connection_auth_psk: SecretString,

    // TCP listener to create when the client says it wants the default one.
    pub default_tcp_listener: SocketAddr,

    // Whether to allow clients to create additional TCP listeners.
    pub clients_additional_listeners: bool,
}

impl ServerAppData {
    pub fn new(connection_auth_psk: SecretString, default_tcp_listener: SocketAddr) -> Self {
        ServerAppData {
            connection_auth_psk,
            default_tcp_listener,
            clients_additional_listeners: false,
        }
    }
}

// Implementing the Display trait for ServerAppData.
impl fmt::Display for ServerAppData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ServerAppData {{ connection_auth_psk: [REDACTED] }}",)
    }
}

// Storage for application data for handler functions.
#[derive(Clone, Debug)]
pub struct ClientAppData {
    pub connection: Arc<Mutex<Option<quinn::Connection>>>,
    pub connection_auth_psk: SecretString,

    // The destination address to forward packets to.
    pub forward_destination: SocketAddr,
}

impl ClientAppData {
    pub fn new(connection_auth_psk: SecretString, forward_destination: SocketAddr) -> Self {
        ClientAppData {
            connection: Arc::new(Mutex::new(None)),
            connection_auth_psk,

            forward_destination,
        }
    }
}

// Implementing the Display trait for ClientAppData.
impl fmt::Display for ClientAppData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ClientAppData {{ connection: {:?}, connection_auth_psk: [REDACTED], destination: {} }}", self.connection, self.forward_destination)
    }
}
