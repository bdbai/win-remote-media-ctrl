use std::io;

use aes_gcm::{
    aead::AeadMutInPlace, AeadCore, Aes128Gcm, Key as AesGcmKey, KeyInit, Nonce, Tag as AeadTag,
};
use axum::extract::ws;

type Aes128GcmKey = AesGcmKey<Aes128Gcm>;
type Aes128GcmTag = AeadTag<<Aes128Gcm as AeadCore>::TagSize>;

pub(crate) struct Crypto {
    upload_aes: Aes128Gcm,
    upload_nonce: Nonce<<Aes128Gcm as AeadCore>::NonceSize>,
    download_aes: Aes128Gcm,
    download_nonce: Nonce<<Aes128Gcm as AeadCore>::NonceSize>,
}

impl Crypto {
    pub(crate) fn new(upload_key: &Aes128GcmKey, download_key: &Aes128GcmKey) -> Self {
        let upload_aes = Aes128Gcm::new(upload_key);
        let download_aes = Aes128Gcm::new(download_key);
        Self {
            upload_aes,
            upload_nonce: Nonce::default(),
            download_aes,
            download_nonce: Nonce::default(),
        }
    }
}

impl Crypto {
    pub(crate) fn decrypt_in_place<'d>(
        &mut self,
        data: &'d mut [u8],
    ) -> super::WebSocketResult<&'d mut [u8]> {
        let tag_pos = data
            .len()
            .checked_sub(Aes128GcmTag::default().len())
            .ok_or_else(generate_crypto_error)?;
        let (data, tag) = data.split_at_mut(tag_pos);
        let tag = Aes128GcmTag::from_slice(tag);
        let res = self
            .upload_aes
            .decrypt_in_place_detached(&self.upload_nonce, &[], data, tag);
        increase_nonce(&mut self.upload_nonce);
        match res {
            Ok(()) => Ok(data),
            Err(_) => Err(generate_crypto_error()),
        }
    }
    pub(crate) fn encrypt(&mut self, data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(data.len() + Aes128GcmTag::default().len());
        buf.extend_from_slice(data);
        let tag = self
            .download_aes
            .encrypt_in_place_detached(&self.download_nonce, &[], &mut buf[..])
            .expect("enc data too large");
        increase_nonce(&mut self.download_nonce);
        buf.extend_from_slice(&tag);
        buf
    }
}

fn generate_crypto_error() -> super::WebSocketError {
    super::WebSocketError {
        close_frame: ws::CloseFrame {
            code: 3004,
            reason: "crypto error".into(),
        },
        error: io::Error::new(io::ErrorKind::InvalidData, "crypto error").into(),
    }
}

fn increase_nonce(nonce: &mut Nonce<<Aes128Gcm as AeadCore>::NonceSize>) {
    let mut c = 1;
    for i in 0..nonce.len() {
        c += nonce[i] as u16;
        nonce[i] = c as u8;
        c >>= 8;
    }
    if c > 0 {
        panic!("nonce overflow. potential nonce reuse");
    }
}
