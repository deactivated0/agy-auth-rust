use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::{Aead,Payload}};
use anyhow::{anyhow,bail,Context,Result};
use base64::{engine::general_purpose::STANDARD,Engine};
use rand::{rngs::OsRng,RngCore};
use std::{fs,path::Path,thread,time::{Duration,Instant}};
use crate::paths::{atomic_write,secure_dir,read_limit};
const PREFIX:&str="agy-auth-encrypted:v1:";

pub fn encrypt(key_path:&Path,plain:&[u8])->Result<Vec<u8>>{
    let key=key_file(key_path,true)?;
    let cipher=Aes256Gcm::new_from_slice(&key).map_err(|_|anyhow!("invalid AES key"))?;
    let mut nonce=[0u8;12]; OsRng.fill_bytes(&mut nonce);
    let ciphertext=cipher.encrypt(Nonce::from_slice(&nonce),Payload{msg:plain,aad:PREFIX.as_bytes()}).map_err(|_|anyhow!("snapshot encryption failed"))?;
    let mut sealed=Vec::with_capacity(12+ciphertext.len());sealed.extend_from_slice(&nonce);sealed.extend_from_slice(&ciphertext);
    Ok(format!("{PREFIX}{}",STANDARD.encode(sealed)).into_bytes())
}
pub fn decrypt(key_path:&Path,data:&[u8])->Result<Vec<u8>> {
    let trimmed=std::str::from_utf8(data).ok().map(str::trim).unwrap_or("");
    if !trimmed.starts_with(PREFIX) {return Ok(data.to_vec());}
    let key=key_file(key_path,false)?;
    let ciphertext=STANDARD.decode(&trimmed[PREFIX.len()..]).context("invalid encrypted snapshot encoding")?;
    if ciphertext.len()<28 {bail!("truncated encrypted snapshot");}
    let cipher=Aes256Gcm::new_from_slice(&key).map_err(|_|anyhow!("invalid AES key"))?;
    cipher.decrypt(Nonce::from_slice(&ciphertext[..12]),Payload{msg:&ciphertext[12..],aad:PREFIX.as_bytes()}).map_err(|_|anyhow!("snapshot decryption failed; check encryption.key"))
}
pub fn encrypted(data:&[u8])->bool{data.starts_with(PREFIX.as_bytes())}

fn read_key(path:&Path)->Result<[u8;32]>{
    let raw=read_limit(path,512)?;
    let key=STANDARD.decode(String::from_utf8_lossy(&raw).trim())?;
    let key:[u8;32]=key.try_into().map_err(|_|anyhow!("encryption key must decode to exactly 32 bytes"))?;
    Ok(key)
}
fn key_file(path:&Path,create:bool)->Result<[u8;32]>{
    match read_key(path){Ok(k)=>return Ok(k),Err(e) if path.exists()=>return Err(e),_=>()}
    if !create {bail!("encryption.key is missing, cannot decrypt saved credentials");}
    secure_dir(path.parent().ok_or_else(||anyhow!("key path has no parent"))?)?;
    let lock=path.with_file_name("encryption.key.lock");
    let start=Instant::now();
    loop {
        match fs::create_dir(&lock){
            Ok(())=>{
                struct Guard<'a>(&'a Path);
                impl Drop for Guard<'_>{fn drop(&mut self){let _=fs::remove_dir(self.0);}}
                let _guard=Guard(&lock);
                if path.exists(){return read_key(path);}
                let mut key=[0u8;32]; OsRng.fill_bytes(&mut key);
                atomic_write(path,STANDARD.encode(key).as_bytes())?;
                return Ok(key);
            }
            Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{
                if path.exists(){return read_key(path);}
                if start.elapsed()>Duration::from_secs(2){bail!("encryption key creation locked");}
                thread::sleep(Duration::from_millis(10));
            }
            Err(e)=>return Err(e.into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatible_envelope_roundtrip() {
        let dir=tempfile::tempdir().unwrap();let key=dir.path().join("encryption.key");
        let p=b"{\"token\":\"secret\"}";
        let sealed=encrypt(&key,p).unwrap();
        assert!(encrypted(&sealed));assert_ne!(sealed,p);
        assert_eq!(decrypt(&key,&sealed).unwrap(),p);
        let mut tampered=sealed.clone();*tampered.last_mut().unwrap()^=1;
        assert!(decrypt(&key,&tampered).is_err());
    }
}
