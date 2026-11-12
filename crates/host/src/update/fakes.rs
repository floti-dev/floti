//! Fakes the tests of `floti update` and `floti add` share: a minisign key
//! made for the test, and a network that answers fixed bodies by address.

use std::collections::HashMap;
use std::io::{self, Cursor};
use std::sync::Mutex;

use url::Url;

use super::network::{FetchError, Network};

/// A minisign key pair made for the test.
pub struct Signer {
    pair: minisign::KeyPair,
}

impl Signer {
    pub fn new() -> Self {
        Self {
            pair: minisign::KeyPair::generate_unencrypted_keypair().expect("make a key pair"),
        }
    }

    pub fn public(&self) -> minisign_verify::PublicKey {
        let text = self.pair.pk.to_box().expect("box the key").into_string();
        minisign_verify::PublicKey::decode(&text).expect("read the key back")
    }

    pub fn sign(&self, bytes: &[u8], comment: &str) -> Vec<u8> {
        minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            Cursor::new(bytes),
            Some(comment),
            None,
        )
        .expect("sign")
        .into_string()
        .into_bytes()
    }
}

/// What the fake network answers for an address.
#[derive(Clone)]
pub enum Served {
    Body(Vec<u8>),
    /// The first bytes, then the connection drops.
    CutAfter(Vec<u8>, usize),
}

#[derive(Default)]
pub struct FakeNetwork {
    served: Mutex<HashMap<String, Served>>,
    asked: Mutex<Vec<String>>,
}

impl FakeNetwork {
    pub fn serve(&self, url: &str, served: Served) {
        self.served.lock().unwrap().insert(url.to_owned(), served);
    }

    pub fn forget(&self, url: &str) {
        self.served.lock().unwrap().remove(url);
    }

    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Network for FakeNetwork {
    fn get(
        &self,
        url: &Url,
        limit: u64,
        sink: &mut dyn FnMut(&[u8]) -> io::Result<()>,
    ) -> Result<(), FetchError> {
        self.asked.lock().unwrap().push(url.to_string());
        let served = self.served.lock().unwrap().get(url.as_str()).cloned();
        match served {
            None => Err(FetchError::NotFound),
            Some(Served::Body(body)) => {
                if body.len() as u64 > limit {
                    return Err(FetchError::TooLarge(limit));
                }
                // In two pieces, as a network hands a body over.
                let (first, second) = body.split_at(body.len() / 2);
                sink(first).map_err(FetchError::Sink)?;
                sink(second).map_err(FetchError::Sink)
            }
            Some(Served::CutAfter(body, kept)) => {
                sink(&body[..kept]).map_err(FetchError::Sink)?;
                Err(FetchError::Cut("the connection closed".into()))
            }
        }
    }
}
