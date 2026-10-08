//! API キーの保管（T3-5、D066）：OS のキーチェーン（macOS のキーチェーン、Windows の資格情報マネージャー）に、
//! 行き先（[`super::Connection::endpoint`]）ごとに置く。作品フォルダにも端末の設定にも書かず、フロントにも渡さない。

use std::collections::BTreeMap;
use std::sync::Mutex;

/// キーチェーンのサービス名。
pub const KEYCHAIN_SERVICE: &str = "scn-studio";

/// キーチェーンを使えなかった。文にはキーを含めない。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct KeyError(String);

/// キーの置き場所。アプリは [`SystemKeys`]、テストは [`MemoryKeys`] を使う。
pub trait KeyStore {
    /// 行き先のキー。無ければ `None`。
    fn get(&self, endpoint: &str) -> Result<Option<String>, KeyError>;
    fn set(&self, endpoint: &str, key: &str) -> Result<(), KeyError>;
    /// 消す。もう無ければ何もしない。
    fn delete(&self, endpoint: &str) -> Result<(), KeyError>;
}

/// 入力されたキーを確かめる（前後の空白を除く。空や改行を含むものは断る）。
pub fn clean_key(key: &str) -> Result<&str, KeyError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(KeyError("キーが空です".to_owned()));
    }
    if key.chars().any(char::is_control) {
        return Err(KeyError("キーに改行や制御文字が入っています".to_owned()));
    }
    Ok(key)
}

/// OS のキーチェーン。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemKeys;

fn keychain_error(e: &keyring::Error) -> KeyError {
    // 読めなかった中身（BadEncoding など）を文に入れない。
    let reason = match e {
        keyring::Error::NoStorageAccess(_) => {
            "キーチェーンを開けません（ロックされているか、使うことが許されていません）".to_owned()
        }
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => {
            "キーチェーンの中身を読めません".to_owned()
        }
        keyring::Error::PlatformFailure(inner) => format!("キーチェーンを使えません：{inner}"),
        keyring::Error::NoDefaultStore => "この OS のキーチェーンを使えません".to_owned(),
        _ => "キーチェーンを使えません".to_owned(),
    };
    KeyError(reason)
}

fn entry(endpoint: &str) -> Result<keyring::Entry, KeyError> {
    keyring::Entry::new(KEYCHAIN_SERVICE, endpoint).map_err(|e| keychain_error(&e))
}

impl KeyStore for SystemKeys {
    fn get(&self, endpoint: &str) -> Result<Option<String>, KeyError> {
        match entry(endpoint)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_error(&e)),
        }
    }

    fn set(&self, endpoint: &str, key: &str) -> Result<(), KeyError> {
        let key = clean_key(key)?;
        entry(endpoint)?
            .set_password(key)
            .map_err(|e| keychain_error(&e))
    }

    fn delete(&self, endpoint: &str) -> Result<(), KeyError> {
        match entry(endpoint)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_error(&e)),
        }
    }
}

/// メモリの中のキー（テスト用）。
#[derive(Debug, Default)]
pub struct MemoryKeys(Mutex<BTreeMap<String, String>>);

impl MemoryKeys {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, String>>, KeyError> {
        self.0
            .lock()
            .map_err(|_| KeyError("キーの置き場所を使えません".to_owned()))
    }
}

impl KeyStore for MemoryKeys {
    fn get(&self, endpoint: &str) -> Result<Option<String>, KeyError> {
        Ok(self.lock()?.get(endpoint).cloned())
    }

    fn set(&self, endpoint: &str, key: &str) -> Result<(), KeyError> {
        let key = clean_key(key)?;
        self.lock()?.insert(endpoint.to_owned(), key.to_owned());
        Ok(())
    }

    fn delete(&self, endpoint: &str) -> Result<(), KeyError> {
        self.lock()?.remove(endpoint);
        Ok(())
    }
}
