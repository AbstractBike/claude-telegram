use anyhow::Result;
use metrics::counter;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("access denied: agent '{agent}' cannot read '{secret}'")]
    Denied { agent: String, secret: String },
    #[error("secret not found: '{0}'")]
    NotFound(String),
}

#[derive(Debug, Deserialize)]
struct Policy {
    agents: HashMap<String, AgentPolicy>,
}

#[derive(Debug, Deserialize)]
struct AgentPolicy {
    allowed_secrets: Vec<String>,
}

pub struct Vault {
    root: PathBuf,
    policy: Policy,
}

impl Vault {
    pub fn load(root: &str) -> Result<Self> {
        let root = PathBuf::from(root);
        let policy_path = root.join("policy.toml");
        let policy_str = std::fs::read_to_string(&policy_path)?;
        let policy: Policy = toml::from_str(&policy_str)?;
        Ok(Self { root, policy })
    }

    pub fn read_secret(&self, agent: &str, secret: &str) -> Result<String, PolicyError> {
        let agent_policy = self.policy.agents.get(agent).ok_or_else(|| {
            self.record_access(agent, secret, "denied");
            PolicyError::Denied {
                agent: agent.to_string(),
                secret: secret.to_string(),
            }
        })?;

        if !agent_policy.allowed_secrets.iter().any(|s| s == secret) {
            self.record_access(agent, secret, "denied");
            return Err(PolicyError::Denied {
                agent: agent.to_string(),
                secret: secret.to_string(),
            });
        }

        let secret_path = self.root.join("vault").join(secret);
        let content = std::fs::read_to_string(&secret_path)
            .map_err(|_| PolicyError::NotFound(secret.to_string()))?;

        self.record_access(agent, secret, "granted");

        Ok(content)
    }

    pub fn read_public_key(&self, agent: &str) -> Result<String> {
        // Support both key layouts:
        //   {root}/keys/{agent}.pub          — flat format used in tests
        //   {root}/keys/{agent}/pubkey.txt   — directory format used in production
        let flat = self.root.join("keys").join(format!("{agent}.pub"));
        let nested = self.root.join("keys").join(agent).join("pubkey.txt");
        let path = if flat.exists() { flat } else { nested };
        Ok(std::fs::read_to_string(path)?.trim().to_string())
    }

    fn record_access(&self, agent: &str, secret: &str, result: &str) {
        counter!(
            "claude_chat_mcp_secret_requests_total",
            "agent" => agent.to_string(),
            "secret" => secret.to_string(),
            "result" => result.to_string()
        )
        .increment(1);

        tracing::info!(
            service = "claude-chat",
            event = "secret_access",
            agent = agent,
            secret = secret,
            result = result
        );
    }
}

// --- Age X25519 encryption/decryption ---

/// Encrypt plaintext with an age X25519 public key
pub fn encrypt_for_agent(plaintext: &str, pubkey_str: &str) -> Result<Vec<u8>> {
    let pubkey: age::x25519::Recipient = pubkey_str
        .parse()
        .map_err(|e: &str| anyhow::anyhow!("invalid public key: {e}"))?;

    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&pubkey as &dyn age::Recipient))
            .map_err(|e| anyhow::anyhow!("failed to create encryptor: {e}"))?;

    let mut output = Vec::new();
    let mut writer = encryptor.wrap_output(&mut output)?;
    writer.write_all(plaintext.as_bytes())?;
    writer.finish()?;
    Ok(output)
}

/// Decrypt ciphertext with an age X25519 identity (private key)
pub fn decrypt_with_identity(ciphertext: &[u8], identity_str: &str) -> Result<String> {
    let identity: age::x25519::Identity = identity_str
        .parse()
        .map_err(|e: &str| anyhow::anyhow!("invalid identity: {e}"))?;

    let decrypted =
        age::decrypt(&identity, ciphertext).map_err(|e| anyhow::anyhow!("decryption failed: {e}"))?;

    String::from_utf8(decrypted).map_err(|e| anyhow::anyhow!("invalid UTF-8 in decrypted data: {e}"))
}
