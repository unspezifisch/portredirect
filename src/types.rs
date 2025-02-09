use std::sync::{Arc, Mutex};

use secrecy::SecretString;

// Storage for application data for handler functions.
#[derive(Clone)]
pub struct PRAppData {
    quinn_connection: Arc<Mutex<Option<quinn::Connection>>>,
    connection_auth_psk: SecretString,
}

impl PRAppData {
    pub fn new(connection_auth_psk: SecretString) -> Self {
        PRAppData {
            quinn_connection: Arc::new(Mutex::new(None)),
            connection_auth_psk,
        }
    }
}
