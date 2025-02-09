use std::fmt;
use std::net::SocketAddr;
use std::sync::{atomic::AtomicUsize, Arc, Mutex};

use secrecy::SecretString;

// Storage for application data for handler functions.
#[derive(Clone, Debug)]
pub struct ServerAppData {
    pub connection: Arc<Mutex<Option<quinn::Connection>>>,
    pub connection_auth_psk: SecretString,
}

impl ServerAppData {
    pub fn new(connection_auth_psk: SecretString) -> Self {
        ServerAppData {
            connection: Arc::new(Mutex::new(None)),
            connection_auth_psk,
        }
    }
}

// Implementing the Display trait for ServerAppData.
impl fmt::Display for ServerAppData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ServerAppData {{ connection: {:?}, connection_auth_psk: [REDACTED] }}",
            self.connection
        )
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
